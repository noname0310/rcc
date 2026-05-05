# Project probe plan template

Copy this to `real_world/projects/NN-name/plan.md` after cloning the project.
Fill it from the fetched source tree; do not invent a detailed plan before
reading the actual upstream build files.

## Source snapshot

- Project:
- Upstream URL:
- Clone/archive command:
- Resolved commit or checksum:
- Date fetched:

## Upstream build entry points

- Build files inspected:
- Smallest compile target:
- Smallest runnable target:
- Required generated config:

## Baseline oracle

- Host compiler:
- Host compile command:
- Host run command:
- Expected exit status:
- Expected stdout/stderr:

## rcc probe

- `rcc` compile command:
- Link command, if separate:
- Run command:
- Expected comparison:

## Allowed local adaptation

- Wrapper scripts:
- Build-script-only patches:
- Generated config files:

## Disallowed adaptation checklist

- [ ] No upstream `.c` file modified
- [ ] No upstream `.h` file modified
- [ ] No failing upstream test deleted
- [ ] No runtime oracle weakened to hide an `rcc` bug

## Failure log

Record each failure here before fixing anything.

| ID | Command | Symptom | Classification | Follow-up task |
| --- | --- | --- | --- | --- |

## Exit criteria

- [ ] Host baseline built
- [ ] Host baseline run recorded, when applicable
- [ ] `rcc` build attempted
- [ ] `rcc` run compared with baseline, when applicable
- [ ] Compiler bugs have minimized regressions
- [ ] `RESULTS.md` updated

