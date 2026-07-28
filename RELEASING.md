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

3. Commit the version, create an annotated `vX.Y.Z` tag and push both:

   ```bash
   git tag -a vX.Y.Z -m "tgb-cli vX.Y.Z"
   git push origin main vX.Y.Z
   ```

4. The `Release` GitHub Actions workflow builds `arm64` and `x86_64`, combines
   them into a universal macOS executable and uploads the versioned archive to
   the GitHub release.

5. Download the release archive and verify its checksum:

   ```bash
   curl -L -o tgb-cli-vX.Y.Z.tar.gz \
     https://github.com/tinylion1024/tgb-cli/releases/download/vX.Y.Z/tgb-cli-vX.Y.Z-universal-apple-darwin.tar.gz
   shasum -a 256 tgb-cli-vX.Y.Z.tar.gz
   ```

6. Update the version, URL and SHA-256 in `Formula/tgb-cli.rb` in
   `tinylion1024/homebrew-tap`, then run:

   ```bash
   brew audit --strict tinylion1024/tap/tgb-cli
   brew reinstall --build-from-source tinylion1024/tap/tgb-cli
   tgb --help
   ```

7. Commit and push the Formula update.
