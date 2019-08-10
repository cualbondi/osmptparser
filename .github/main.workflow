workflow "CI" {
  on = "push"
  resolves = ["CI"]
}

action "CI" {
  uses = "icepuma/rust-action@master"
  args = "cargo fmt -- --check && cargo clippy -- -D warnings -A clippy::ptr-arg && cargo test"
}
