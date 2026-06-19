use vergen_gix::{Build, Cargo, Emitter, Gix, Rustc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Migrations live in `atc-store-pg/migrations/` (#169). The
    // `cargo:rerun-if-changed=migrations` anchor that lived here moved
    // into `backend/crates/atc-store-pg/build.rs`, alongside the
    // `sqlx::migrate!()` call site. `sqlx::migrate!`'s `include_str!`
    // tracking catches *modifications* to existing files but not
    // *additions* of new migration files, so the explicit anchor in the
    // store-pg crate is load-bearing.
    Ok(Emitter::default()
        .add_instructions(&Build::all_build())?
        .add_instructions(&Cargo::all_cargo())?
        .add_instructions(&Gix::all_git())?
        .add_instructions(&Rustc::all_rustc())?
        .emit()?)
}
