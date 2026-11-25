# Animation graph decoder

`decode_graph.py` decodes the character animation state machines (`.afsmb`) into text: every state's transitions with their events and conditions, every condition's class and settings, and every state's class and state tasks. The format and what the decoded graphs show about the game are in [`docs/engine/character/animation-graph.md`](../../docs/engine/character/animation-graph.md).

The tool uses only the Python standard library, and borrows the archive reader from [`tools/sights`](../sights). The format's property name hashes are in `afsmb.py`, named after what the engine's loader reads each into.

## Prerequisites

- Python 3.10 or newer.
- The game install. The tool reads the graphs from `archives_win64/` and `patch_win64/` under it, preferring the patch copy, which is the one the game loads.
- The symbol dump, for names. The condition and state classes are stored as hashes of their class names, which the tool recovers by hashing every class name the dump has a file for; the state task types are stored as the hashes the engine's state-task registration table pairs with each `CStateTask_<NAME>` string, which the tool reads from the dump's `_data.cpp`. Without the dump, classes and task types print as hashes.

The paths follow the same environment variables as the reverse-engineering skill, loaded from `.env` by direnv: `JC3_GAME_DIR` and `JC3_DEBUG_BUILD_WITH_SYMBOLS_DUMP`.

## Usage

```sh
# Decode Rico's four graphs.
python3 decode_graph.py --out /tmp/anim_graph

# Another graph from the archives, by its path.
python3 decode_graph.py --out /tmp/anim_graph --graph animations/jc_statemachine/main_characters/rico/external/use_item_tape.afsmb

# A graph already extracted to disk.
python3 decode_graph.py --out /tmp/anim_graph --file rico_base.afsmb
```

`--game` defaults to `$JC3_GAME_DIR`, then to the default Steam library path, and `--dump` to `$JC3_DEBUG_BUILD_WITH_SYMBOLS_DUMP`. `--graph` and `--file` repeat. The decoded graphs are game content, so write them outside the repository.

To extract a graph to disk instead, use the Gibbed tools' unpacker as the reverse-engineering skill describes (including its registry workaround), filtering on the graph's name, on the archive the file lists place it in. Rico's graphs are in `patch_win64/game1`:

```sh
mono Gibbed.JustCause3.Unpack.exe -f 'main_characters/rico/rico_' "$JC3_GAME_DIR/patch_win64/game1.tab" /tmp/extract
```

Do not use `Gibbed.JustCause3.ConvertProperty` output for the graph: its XML sorts each object's children by name, and the graph's tables index the children in file order.

## Output

Three files per graph, named after it:

- `<graph>.graph.txt`: `INITIAL <state>`, then the global (`GLOBAL`) and external (`EXTERNAL`) transitions, then one line per state transition, `STATE [EVENT] conditions -> DESTINATION`. `!` marks a condition that must be false; conditions are joined with `&` when all must hold and with `|` when any may. A state without transitions has a line of its own name. `ACT_UPDATE` transitions are evaluated every update; the others fire on their act.
- `<graph>.conds.txt`: one line per condition: its name, its class, and its other properties as `name=value`, a property whose name string is not known shown by its hash.
- `<graph>.tasks.txt`: one line per state: its name, its class, and its state tasks in order, each `TYPE(properties)`, or `-` for none.

A run over Rico's four graphs takes about a second.
