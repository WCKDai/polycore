# Dependency advisory decisions — 2026-10-30

OSV-Scanner is gated to the workspace `Cargo.lock`. The nested `vendor/*/Cargo.lock` files are
historical inputs, not the built graph; scanning them would report versions PolyCore does not use.

The three remaining root-lock advisories are **explicit, time-boxed acceptances**:

| Advisory | Package | Decision | Why / next action |
|---|---|---|---|
| RUSTSEC-2026-0196 | cgmath 0.18 | Accept to 2027-01-31 | Unmaintained geometry dependency; replacement is a PolyCore/Truck migration. |
| RUSTSEC-2026-0197 | cgmath 0.18 | Accept to 2027-01-31 | `swap_columns` UB requires identical indices; API not called in PolyCore. |
| RUSTSEC-2024-0370 | proc-macro-error 1.0.4 | Accept to 2027-01-31 | Build-time proc-macro only, from the pinned Truck derivation chain. |

Owner decision: the 2026-10-30 engineering task explicitly authorised resolving or accepting the
remaining advisories. Review by **2027-01-31**, or earlier if a fixed release/upstream replacement
lands. A new advisory fails the OSV gate rather than inheriting an old exception.
