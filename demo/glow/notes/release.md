# Release checklist

Everything that has to be true before **v0.1.0** ships.

## Gates

| gate    | command                          | status |
| ------- | -------------------------------- | ------ |
| format  | `cargo fmt --check`              | ok     |
| lint    | `cargo clippy -- -D warnings`    | ok     |
| tests   | `cargo test --workspace`         | ok     |
| package | `cargo package --workspace`      | ok     |

## Tag

```sh
cargo release tag --workspace --execute
git push origin v0.1.0
```

> One tag for the whole workspace: `v0.1.0`, never `anina-v0.1.0`.

## After

- [x] gatecheck green on main
- [ ] crates.io page reads well
- [ ] announce
