# Locker source inspection

These tools inspect selected shipped location and weapon files. They reuse `tools/sights/archives.py` for archive precedence and `tools/sights/rtpc.py` for property containers. Base and patch archives remain read-only. The scanner reads only the supplied paths and their embedded members.

`inspect_sources.py` emits JSON with archive provenance, RTPC node classes and properties, embedded model references, and static model-collection instances. Standalone replacements take precedence over embedded RTPC and model-collection members. Static instance records include the original collection index, the model name and hash, the world matrix, the saved flags, and the world bounds. `instance_count` preserves the original collection count when `--match` filters the emitted instances.

The parser supports the shipped model-collection format used by the selected Massos locations. It rejects unsupported versions, invalid name indices, truncated arrays, malformed names, and non-finite world matrices. These static records are render instances. They are distinct from the location's placement-rule weapon actors.

## File discovery

The scanner searches normalized resolved file-list names without extracting assets:

```sh
python3 tools/locker_sources/inspect_sources.py --search 'outpost_guardia_massos.*\.nl$'
```

`JC3_GIBBED_TOOLS` supplies the default file-list directory. `--file-lists` overrides it.

## Selective inspection

`JC3_GAME_DIR` supplies the installation directory. `--game-dir` overrides it. A selected location container includes its embedded property files and static model collection:

```sh
python3 tools/locker_sources/inspect_sources.py \
  --match 'w301_grenade_frag_body.lod|WeaponRack|CWeaponPlacementRule' \
  locations/region_02/military/outposts/outpost_04/outpost_guardia_massos_03.nl \
  > target/locker-sources-massos03.json
```

A weapon entity can be inspected with its `.ee` path. The scanner uses the effective standalone property member when a patch overrides the embedded copy.

`--match` filters each complete JSON node or model-instance record with a case-insensitive regular expression. Archive and member provenance remain in the output. A model hash alone does not establish an interactive source. Runtime source identity also requires the loaded collection, its lifetime, and the exact member index.

## Optional geometry plot

`plot_rack.py` plots the static mesh positions referenced by the military cabinet mesh. It requires NumPy and Matplotlib. The scanner and unit tests use only the Python standard library.

## Tests

```sh
python3 -m unittest discover -s tools/locker_sources -p 'test_*.py' -v
```

The tests use synthetic dictionaries and archive stubs. They do not require the game or a running process.
