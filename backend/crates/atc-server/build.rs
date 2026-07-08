use vergen_gix::{Build, Cargo, Emitter, Gix, Rustc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Migrations live in `atc-store-pg/migrations/` (#169). The
    // `cargo:rerun-if-changed=migrations` anchor that lived here moved
    // into `backend/crates/atc-store-pg/build.rs`, alongside the
    // `sqlx::migrate!()` call site. `sqlx::migrate!`'s `include_str!`
    // tracking catches *modifications* to existing files but not
    // *additions* of new migration files, so the explicit anchor in the
    // store-pg crate is load-bearing.
    // `Gix::all_git()` defaults `describe`'s `tags` flag to false, i.e. plain
    // `git describe` semantics: only annotated tags count. Only `v0.2.0` was
    // ever created as an annotated tag; every release since (`v0.3.0`,
    // `v0.4.0`, the `atc-*` chart tags) is lightweight, so the embedded
    // VERGEN_GIT_DESCRIBE kept resolving against `v0.2.0` no matter how far
    // HEAD moved. `describe(true, false, None)` switches on `--tags`
    // (lightweight included), matching what `atc-*`/`v*` tagging actually is.
    Ok(Emitter::default()
        .add_instructions(&Build::all_build())?
        .add_instructions(&Cargo::all_cargo())?
        .add_instructions(&Gix::all().describe(true, false, None).build())?
        .add_instructions(&Rustc::all_rustc())?
        .emit()?)
}
