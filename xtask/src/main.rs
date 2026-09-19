// Returning the result rather than dropping it: a failed bundle now exits
// non-zero instead of reporting success, and `cargo clippy -- -D warnings`
// stops failing on the discarded `Result`.
fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    nih_plug_xtask::main()?;
    Ok(())
}
