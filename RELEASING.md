# Release process

`tgb-cli` uses semantic version tags and is distributed through
[`tinylion1024/homebrew-tap`](https://github.com/tinylion1024/homebrew-tap).

## Publish a version

1. Update the version in `Cargo.toml` and run `cargo check` to refresh
   `Cargo.lock`.
2. Run the local verification suite:

   ```bash
   cargo fmt --check
   cargo clippy --locked --all-targets --all-features -- -D warnings
   cargo test --locked --all-targets
   cargo package --locked
   ```

3. Commit the version, create an annotated `vX.Y.Z` tag and push both.
4. Create the GitHub release from that tag:

   ```bash
   gh release create vX.Y.Z --verify-tag --generate-notes
   ```

5. Download the immutable source archive and calculate its checksum:

   ```bash
   curl -L -o tgb-cli-vX.Y.Z.tar.gz \
     https://github.com/tinylion1024/tgb-cli/archive/refs/tags/vX.Y.Z.tar.gz
   shasum -a 256 tgb-cli-vX.Y.Z.tar.gz
   ```

6. Update `Formula/tgb-cli.rb` in `tinylion1024/homebrew-tap` with the new
   URL and SHA-256, then run:

   ```bash
   brew audit --strict tinylion1024/tap/tgb-cli
   brew reinstall --build-from-source tinylion1024/tap/tgb-cli
   tgb --help
   ```

7. Commit and push the Formula update.
