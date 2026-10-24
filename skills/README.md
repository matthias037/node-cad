# Agent skills

Skills for AI agents that model in NØDE CAD through its [MCP server](../docs/mcp.md). Each folder is
one skill in the [Agent Skills](https://agentskills.io) format: a `SKILL.md` with a name and a
description that tell the agent when to load it, and an optional supporting reference file.

| Skill | Loads when the agent |
|---|---|
| [`node-cad-modeling`](node-cad-modeling/SKILL.md) | builds a new part (enclosure, bracket, plate, flange, …) |
| [`node-cad-editing`](node-cad-editing/SKILL.md) | changes, fixes or extends an existing design |
| [`node-cad-checking`](node-cad-checking/SKILL.md) | builds, changes or ports a design, next to the two above: parameter ranges, every allowed value, exact STEP comparison, regression tests, designs kept in git |
| [`node-cad-mechanisms`](node-cad-mechanisms/SKILL.md) | builds, drives or animates an assembly: components, joints, motion links, closed loops such as actuators, contact sets, motion studies |

They teach the agent to:
- plan the part as numbers
- build it in a few `batch` calls with parametric positions
- verify every stage with `measure` against values it computed itself
- skip the large replies it doesn't need, such as the unfiltered command catalog and full topology dumps

They also list the traps that otherwise cost failed calls. Examples are the sketch axes of the XZ plane, edge picks that snap to the nearest edge, and cuts that miss but still report success.

They are written for NØDE CAD 0.8 and later; `node-cad-mechanisms` for `main` after 0.11.1 (the next release), which counts the degrees of freedom of closed loops.

## Install

Claude Code, for all projects:

```sh
git clone --depth 1 https://github.com/matthias037/node-cad
cp -r node-cad/skills/node-cad-* ~/.claude/skills/
```

Windows (PowerShell):

```powershell
git clone --depth 1 https://github.com/matthias037/node-cad
Copy-Item -Recurse node-cad\skills\node-cad-* $HOME\.claude\skills\
```

- **One project only:** copy the folders into that project's `.claude/skills/` instead.
- **Other agents:** for agents that read the Agent Skills format, copy the folders into the skills directory their documentation names.
- **Updating:** copy the folders again after you update NØDE CAD.

## Maintaining

- The worked example in `node-cad-modeling/reference.md` runs in CI. `skill_reference_example_builds_as_documented` in `crates/mcp/src/tests.rs` builds it, checks the documented volume, changes a parameter and checks it again.
- The sketch-plane table is covered by `sketch_plane_axes_match_the_skill_reference` in the same file.
- The worked example in `node-cad-mechanisms/reference.md` runs in CI too: `mechanisms_skill_example_moves_as_documented` builds it, drives the hinge and checks the actuator stroke, the linked panel and the stop against their analytic values.
- `skills_name_only_real_tools_and_commands` checks that every tool and command the skills call exists.
- `node-cad-checking/scripts/tests` checks that skill's helpers, its traps and conventions, and the facts behind the corrections in `node-cad-modeling`: `PYTHONDONTWRITEBYTECODE=1 uv run --no-project --with pytest --with build123d pytest -p no:cacheprovider skills/node-cad-checking/scripts/tests`. The tests run the `node-cad-cli` on `PATH`, not the workspace build, and CI does not run them.
- When a command, a parameter name or a behaviour that these files describe changes, update them in the same PR.
- Facts that describe current limitations, such as edge picks without a distance limit, should go once the limitation is fixed.
