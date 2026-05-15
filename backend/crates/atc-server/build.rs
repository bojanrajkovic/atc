use vergen_gix::{BuildBuilder, CargoBuilder, Emitter, GixBuilder, RustcBuilder};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Migrations live in `atc-store-pg/migrations/` (#169) — the
    // `sqlx::migrate!()` macro inside that crate already emits its own
    // `rerun-if-changed` tracking for the file set, so no anchor is needed
    // here.
    Ok(Emitter::default()
        .add_instructions(&BuildBuilder::all_build()?)?
        .add_instructions(&CargoBuilder::all_cargo()?)?
        .add_instructions(&GixBuilder::all_git()?)?
        .add_instructions(&RustcBuilder::all_rustc()?)?
        .emit()?)
}
