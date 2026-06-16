# Sentinel PR Workflow

Sentinel pull requests must target:

```text
hiltonfam/Project-Sentinel:master
```

Do not open Sentinel development PRs against:

```text
RCGV1/Meshtastic-SAME-EAS-Alerter
```

The upstream repository remains an upstream reference only. The active development repository is the `hiltonfam/Project-Sentinel` fork.

## Automated PR Script

Use the PowerShell helper from a feature branch:

```powershell
.\scripts\create-pr.ps1
```

Optional title:

```powershell
.\scripts\create-pr.ps1 -Title "Phase X: short description"
```

Optional body file:

```powershell
.\scripts\create-pr.ps1 -Title "Phase X: short description" -BodyFile ".\pr-body.md"
```

## Safety Checks

The script refuses to continue unless:

* The current branch is not `master`.
* `origin` points to `hiltonfam/Project-Sentinel`.
* The working tree is clean.
* `cargo fmt --check` passes.
* `cargo check` passes.
* `cargo test` passes.

The script also explicitly refuses the upstream repository:

```text
RCGV1/Meshtastic-SAME-EAS-Alerter
```

After validation, the script pushes the current branch to `origin` and creates a pull request against:

```text
hiltonfam/Project-Sentinel:master
```

## GitHub CLI

If the GitHub CLI (`gh`) is installed, the script uses it to create the PR.

If `gh` is not installed, the script prints the manual fallback URL:

```text
https://github.com/hiltonfam/Project-Sentinel/compare/master...<branch>?expand=1
```

Open that URL in a browser to create the PR manually. Verify the base repository and branch before submitting:

```text
base repository: hiltonfam/Project-Sentinel
base branch: master
```

## Recommended Flow

1. Create or switch to a feature branch.
2. Commit all intended changes.
3. Run:

   ```powershell
   .\scripts\create-pr.ps1 -Title "Phase X: short description"
   ```

4. If `gh` is unavailable, open the printed fallback URL.
5. Confirm the PR targets `hiltonfam/Project-Sentinel:master`.

## Manual Fallback Without the Script

If the script cannot be used, run validation manually:

```powershell
cargo fmt --check
cargo check
cargo test
git push -u origin <branch>
```

Then open:

```text
https://github.com/hiltonfam/Project-Sentinel/compare/master...<branch>?expand=1
```

Before creating the PR, confirm that GitHub shows:

```text
base: hiltonfam/Project-Sentinel:master
compare: hiltonfam/Project-Sentinel:<branch>
```
