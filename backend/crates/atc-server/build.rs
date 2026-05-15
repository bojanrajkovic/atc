use vergen_gix::{BuildBuilder, CargoBuilder, Emitter, GixBuilder, RustcBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Migrations live in `atc-store-pg/migrations/` (#169). The
    // `cargo:rerun-if-changed=migrations` anchor that lived here moved
    // into `backend/crates/atc-store-pg/build.rs`, alongside the
    // `sqlx::migrate!()` call site. `sqlx::migrate!`'s `include_str!`
    // tracking catches *modifications* to existing files but not
    // *additions* of new migration files, so the explicit anchor in the
    // store-pg crate is load-bearing.
    Ok(Emitter::default()
        .add_instructions(&BuildBuilder::all_build()?)?
        .add_instructions(&CargoBuilder::all_cargo()?)?
        .add_instructions(&GixBuilder::all_git()?)?
        .add_instructions(&RustcBuilder::all_rustc()?)?
        .emit()?)
}
