# Security policy

PolyCore is the geometry kernel of Polykrom. Only the latest `main` and the revision pinned by
Polykrom are supported.

Report a vulnerability privately through the repository's **Security** tab, not a public issue.
Include the revision, platform, input file or command, expected/actual behavior and impact. Do not
include private designs or credentials.

The kernel is a hostile-input boundary: design, STEP, STL and 3MF input may crash, allocate
excessively or exercise path/geometry assumptions. Releases must pass the full `cargo xtask ci`,
the security workflow and the recorded performance baseline. Dependency-risk acceptances are
time-boxed in `osv-scanner.toml` and explained in `docs/security/dependency-advisories.md`.
