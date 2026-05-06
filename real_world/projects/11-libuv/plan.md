# 11 -- libuv probe plan

## Source snapshot

- Project: libuv
- Upstream URL: <https://github.com/libuv/libuv>
- Clone/archive command: pending
- Resolved commit or checksum: pending
- Date fetched: pending

## Upstream build entry points

- Build files inspected: pending
- Smallest compile target: pending
- Smallest runnable target: pending
- Required generated config: pending

## Baseline oracle

- Host compiler: pending
- Host compile command: pending
- Host run command: pending
- Expected exit status: pending
- Expected stdout/stderr: pending

## rcc probe

- `rcc` compile command: pending
- Link command, if separate: pending
- Run command: pending
- Expected comparison: pending

## Allowed local adaptation

- Wrapper scripts: pending
- Build-script-only patches: pending
- Generated config files: pending

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
