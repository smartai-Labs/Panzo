# Candidate validation and packaging

All explicit relative paths are resolved against the caller's current directory before a script changes location. Paths with spaces or non-ASCII characters are supported. The historical `target/re-mvp-ready` directory is no longer a default candidate.

Run from the workspace root after source edits have settled:

```powershell
.\scripts\verify-validation.ps1
.\scripts\build-candidate.ps1 -TargetDirectory .\target\review-unified
.\scripts\verify-re-mvp-final.ps1 `
    -Executable .\target\review-unified\release\panzo-cli.exe `
    -FixtureRoot .\path-to-prepared-fixtures `
    -ReportDirectory .\docs\acceptance\current-candidate `
    -OutputDirectory .\.tmp\current-candidate
.\scripts\package-re-mvp.ps1 `
    -BuildDirectory .\target\review-unified\release `
    -AcceptanceReport .\docs\acceptance\current-candidate\final-regression.json `
    -Label review-candidate
```

Report and fixture output directories must be new for each run. `FixtureRoot` must contain the existing F01/F02/F03/F05 fixture layout required by the final regression; this example does not create those media fixtures. The final regression launches real media/UI probes and should run in an authorized interactive desktop session.

`build-candidate.ps1` builds both release binaries and writes `candidate-build.json`. It hashes the files under `crates/` and Cargo/toolchain/format configuration before and after the build, and records both EXE hashes. Changing source/configuration or either EXE invalidates the candidate. Rebuild instead of hand-editing the manifest. These hashes identify local artifacts, not signed provenance or a substitute for manual acceptance.

The final regression selects its nested report by an explicit unique directory, not by timestamps. Reports list every required case. Unexpected child exits, exceptions, timeouts, empty completion evidence, failed nested results, missing reports/cases, or identity changes cause a nonzero script exit after the report is written. Expected fault injection is checked inside its owning child script. `Pass` covers only the report's automated scope; `NotRun`/`Deferred` and manual acceptance are never silently promoted to a release claim.

Packaging requires a complete passing final report and its nested unified report from the same candidate. It checks both EXEs before copying and again in the package. The generated package README points to those exact-binary reports; bundled dated implementation notes and old acceptance material are labeled historical.

`verify-validation.ps1` exercises this machinery using small independent fixtures, including a timed-out child process tree. It does not record the desktop or run GPU tests. Its reports and logs remain under a fresh `.tmp/validation-*` directory for inspection.
