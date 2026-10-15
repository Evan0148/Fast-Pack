# Code Signing Policy for Fast‑Pack
Fast‑Pack utilizes free code‑signing service provided by SignPath Foundation for all official Windows release binaries published on GitHub Releases.

1. Only project maintainers within the LongCold‑ColdEternity‑Team GitHub organization are permitted to submit signing requests.
2. All signing artifacts are built reproducibly from public repository source code via GitHub Actions workflow.
3. Private signing keys are fully managed inside SignPath Foundation HSM hardware security module; no private‑key material is accessible to project members.
4. All signed Windows executables are distributed exclusively through the official GitHub Releases page of this repository.
5. This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.

Free code signing provided by SignPath.io, certificate by SignPath Foundation
