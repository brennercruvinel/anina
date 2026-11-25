//! How the explorer gets about: back to a state it knows, along the edges
//! that lead there, one key at a time, and what it does when the program
//! is not where the map says.

use super::*;

impl Explorer<'_> {
    /// Get the program to state `target`: walk known edges from where it is,
    /// else restart and replay. False when the state can't be reached again.
    pub(super) fn goto(&mut self, target: usize) -> Result<bool> {
        if self.at == Some(target) && self.eng.is_some() {
            return Ok(true);
        }
        if let (Some(at), true) = (self.at, self.eng.is_some())
            && let Some(edges) = self.g.path(at, target)
            && self.walk(&edges).unwrap_or(None) == Some(target)
        {
            return Ok(true);
        }
        // restart: the program's first screen is the start state by
        // definition, even when it differs a little (a clock, a recent list)
        self.g.restarts += 1;
        let s = self.launch()?;
        let root_mask = self.g.states[0].mask.clone();
        match self.match_and_learn(&s, &root_mask) {
            Some(0) | None => self.at = Some(0),
            Some(other) => {
                self.log(format!("the program now starts on s{other}, not s0"));
                self.at = Some(other);
            }
        }
        if target == 0 {
            return Ok(self.at == Some(0));
        }
        let Some(edges) = self.g.path(0, target) else {
            return Ok(false);
        };
        Ok(self.walk(&edges).unwrap_or(None) == Some(target))
    }

    /// Press the keys of `edges` in order, checking each lands where the
    /// graph says. Returns where the program ended up, if known.
    pub(super) fn walk(&mut self, edges: &[usize]) -> Result<Option<usize>> {
        for &ei in edges {
            let (key, from, to) = {
                let e = &self.g.edges[ei];
                (e.key.clone(), e.from, e.to)
            };
            let mask = self.g.states[from].mask.clone();
            let before = self.eng()?.screen()?;
            self.press(&key)?;
            let mut st = self.settle(&mask, Some(&before))?;
            if st.exit.is_none() && to != Some(from) && masker::equal_under(&st.screen, &before, &mask) {
                // a slow edge (it was found with the late wait): wait again
                let end = Instant::now() + self.set.late;
                while Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(40));
                    let s = self.eng()?.screen()?;
                    if !masker::equal_under(&s, &before, &mask) || self.eng()?.exit_code()?.is_some() {
                        st = self.settle(&mask, Some(&before))?;
                        break;
                    }
                }
            }
            if st.exit.is_some() {
                self.drop_engine();
                return Ok(None);
            }
            // the same key from the same screen: the expected screen, unless
            // another one matches exactly (live data moves under busy screens)
            let exact = self.g.states.iter().position(|s| masker::equal_under(&st.screen, &s.screen, &s.mask));
            self.at = match (exact, to) {
                (Some(e), _) => Some(e),
                (None, Some(t)) if self.close_to(&st.screen, t) => Some(t),
                _ => {
                    let here = to.map(|t| self.g.states[t].mask.clone()).unwrap_or(mask);
                    self.match_and_learn(&st.screen, &here)
                }
            };
            if self.at != to {
                let differ = to.map(|t| {
                    let s = &self.g.states[t];
                    s.screen.diff(&st.screen).bits.iter().zip(&s.mask.bits).filter(|(a, m)| **a && !**m).count()
                });
                self.log(format!(
                    "  replay s{from} {key}: expected {}, got {} ({} cells off the expected)",
                    to.map_or("exit".into(), |t| format!("s{t}")),
                    self.at.map_or("an unknown screen".into(), |a| format!("s{a}")),
                    differ.unwrap_or(0)
                ));
                return Ok(self.at);
            }
        }
        Ok(self.at)
    }

    /// Press `key` on state `id` (the program must be there) and record the
    /// edge. Returns where it went (None = exited or untracked). An engine
    /// error (the program died under the key) counts as an exit.
    pub(super) fn try_key(&mut self, id: usize, key: &str, label: &str) -> Result<Option<usize>> {
        self.tried.insert((id, key.to_string()));
        let mask = self.g.states[id].mask.clone();
        let before = self.eng()?.screen()?;
        let start = Instant::now();
        let settled = self.press(key).and_then(|_| self.settle(&mask, Some(&before)));
        let st = match settled {
            Ok(st) => st,
            Err(e) => {
                let code = self.eng.as_mut().and_then(|e| e.exit_code().ok().flatten());
                self.log(format!("  s{id} {key} -> the program went away ({e})"));
                self.g.edges.push(Edge {
                    from: id,
                    to: None,
                    key: key.into(),
                    label: label.into(),
                    latency_ms: None,
                    exit_code: code,
                });
                self.drop_engine();
                return Ok(None);
            }
        };
        if let Some(code) = st.exit {
            self.log(format!("  s{id} {key} -> exit {code}"));
            self.g.edges.push(Edge {
                from: id,
                to: None,
                key: key.into(),
                label: label.into(),
                latency_ms: st.latency,
                exit_code: Some(code),
            });
            self.drop_engine();
            return Ok(None);
        }
        let mut st = st;
        if !label.is_empty() && masker::equal_under(&st.screen, &before, &mask) {
            // an advertised key that did nothing is suspicious: give a slow
            // effect (a handoff to another program, a load) time to land
            let end = Instant::now() + self.set.late;
            while Instant::now() < end {
                std::thread::sleep(Duration::from_millis(40));
                let s = self.eng()?.screen()?;
                if !masker::equal_under(&s, &before, &mask) || self.eng()?.exit_code()?.is_some() {
                    let late = start.elapsed().as_millis() as u64;
                    st = self.settle(&mask, Some(&before))?;
                    st.latency = Some(late);
                    break;
                }
            }
            if let Some(code) = st.exit {
                self.log(format!("  s{id} {key} -> exit {code}"));
                self.g.edges.push(Edge {
                    from: id,
                    to: None,
                    key: key.into(),
                    label: label.into(),
                    latency_ms: st.latency,
                    exit_code: Some(code),
                });
                self.drop_engine();
                return Ok(None);
            }
        }
        if masker::equal_under(&st.screen, &before, &mask) {
            // nothing happened: what flickered meanwhile is this screen's own
            self.learn(id, &st.churn);
            self.g.noops.push((id, key.into()));
            self.last_noop = Some((id, key.to_string(), label.to_string()));
            self.at = Some(id);
            return Ok(Some(id));
        }
        self.last_noop = None;
        let depth = self.g.states[id].depth + 1;
        let mut path = self.g.states[id].path.clone();
        path.push(key.to_string());
        // the new screen's own flicker; the source's mask says nothing about it
        let settled_screen = st.screen.clone();
        match self.place(st.screen, &st.churn, depth, &path)? {
            Some(to) if to == id && self.live_noise(id, &before, &settled_screen)? => {
                self.g.noops.push((id, key.into()));
                self.last_noop = Some((id, key.to_string(), label.to_string()));
                self.at = Some(id);
                Ok(Some(id))
            }
            Some(to) => {
                // to == id: the key changed what this view shows (another
                // plot, the next page), kept as a loop on the screen
                self.g.edges.push(Edge {
                    from: id,
                    to: Some(to),
                    key: key.into(),
                    label: label.into(),
                    latency_ms: st.latency,
                    exit_code: None,
                });
                self.at = Some(to);
                Ok(Some(to))
            }
            None => {
                self.g.skipped.push(Skipped {
                    from: id,
                    key: key.into(),
                    label: label.into(),
                    reason: "--max-states reached".into(),
                });
                self.at = None;
                Ok(None)
            }
        }
    }

    /// Before a key goes in: is the program still on `id`? A screen that
    /// changed by itself after a key "did nothing" means that key was slow
    /// (a handoff to another program, a load): its no-op becomes an edge.
    /// Returns true when the program is where we think.
    pub(super) fn check_drift(&mut self, id: usize) -> Result<bool> {
        let now = self.eng()?.screen()?;
        let st = &self.g.states[id];
        // drift means another view: another key bar, or most of it changed;
        // a busy screen (meters, a process list) moving under us is not drift
        if masker::equal_under(&now, &st.screen, &st.mask)
            || (graph::kind(&hints::parse(&now)) == st.kind && self.close_to(&now, id))
        {
            return Ok(true);
        }
        let mask = self.g.states[id].mask.clone();
        let settled = self.settle(&mask, None)?;
        if settled.exit.is_some() {
            if let Some((from, key, label)) = self.last_noop.take()
                && from == id
            {
                self.g.noops.retain(|(s, k)| !(*s == from && *k == key));
                self.log(format!("  s{from} {key} -> exit {} (late)", settled.exit.unwrap_or(0)));
                self.g.edges.push(Edge { from, to: None, key, label, latency_ms: None, exit_code: settled.exit });
            }
            self.drop_engine();
            return Ok(false);
        }
        match self.last_noop.take() {
            Some((from, key, label)) if from == id => {
                self.g.noops.retain(|(s, k)| !(*s == from && *k == key));
                let depth = self.g.states[from].depth + 1;
                let mut path = self.g.states[from].path.clone();
                path.push(key.clone());
                let to = self.place(settled.screen, &settled.churn, depth, &path)?;
                if let Some(to) = to.filter(|t| *t != from) {
                    self.log(format!("  s{from} {key} -> s{to} (late)"));
                    self.g.edges.push(Edge { from, to: Some(to), key, label, latency_ms: None, exit_code: None });
                }
                self.at = to;
            }
            _ => self.at = self.match_and_learn(&settled.screen, &settled.churn),
        }
        Ok(false)
    }
}
