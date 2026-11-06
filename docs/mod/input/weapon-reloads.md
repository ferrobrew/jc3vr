# Weapon reloads

Every weapon in the game, checked against the true reload ([reload.md](reload.md)): what the game does when it reloads the weapon, what the mod loads, and how a player reloads it in the headset. The headset checklist at the end ticks through them.

## Sources and coverage

The weapons are every weapon entity the archives hold, base game, patch, and DLC, read with the tools in `tools/sights/`: the 40 hand weapons in `01_one_handed` and `02_two_handed` that the Gibbed file lists name, the DLC hand weapons the DLC archives name (the Bavarium Splitter in Sky Fortress and Mech Land Assault, the Lightning Gun in Bavarium Sea Heist), the mounted weapons the player can carry (`04_mounted`, among them Mech Land Assault's power core), the thrown weapons, and the stationary and vehicle weapons. The spawn definitions in `payload/src/director/spawn_defs.json` cover the same base-game set less the cut and test weapons, and none of the DLC. The pre-order DLCs add only vehicles. The grappling hook is not a weapon entity.

For each weapon:

- the class, magazine size, ammunition kind, reload type, magazine prop tag, and model parts come from its entity property file and tuning file ([weapon-data.md](../../engine/gameplay/weapon-data.md));
- the reload clip is the state Rico's upper-body animation graph enters on the reload act for that weapon, decoded with `tools/anim_graph/`; the state's tasks say which of the gun's parts the clip moves to a hand ([animation-graph.md](../../engine/character/animation-graph.md));
- what the mod loads is the weapon's entry in `tools/sights/wells.json`, and how it is reloaded follows from the plan (`reload/plan.rs`). The test `every_weapon_in_the_table_loads_what_the_audit_says` in `plan.rs` holds every entry of the wells table to this page.

The game reloads by magazine (the whole magazine at once) unless noted. The game's reloads that move a part to the hand all move `model.magazine` (or the UPU-210's two cartridges); a weapon without that part shows nothing in Rico's hand during its clip.

## Reloaded by hand

| Weapon | Magazine | Game's reload | Load and action | How to reload it | Unusual |
|---|---|---|---|---|---|
| `w011` Pozhar 98 | 16 | pistols clip; magazine prop | magazine in the grip, slide | eject, take a magazine from the pouch, push it up into the grip base; or push the gun's grip onto the pouch | dual-wielded, each pistol has its own half magazine and its own pouch |
| `w012` Spectre Mark V | 14 | pistols clip; magazine prop | magazine in the grip, slide | as the Pozhar | as the Pozhar |
| `w014` Little General | 12 | revolver clip, the gun's own act opens it; no prop | cylinder | eject (nothing drops), push a cylinder into the rear face of the cylinder | the golden gun; its class is pistol, the table makes it a cylinder; nothing is drawn in the hand |
| `w021` Peacekeeper | 12 | as the Little General | cylinder | as the Little General | as the Little General |
| `w031` Prizrak U4 | 40 | pistols clip; magazine prop | magazine modelled into the body ahead of the grip, charging handle | eject, push a magazine up into the base of the magazine ahead of the grip | the magazine cannot be hidden, so the old one stays drawn |
| `w032` Wraith 2000 | 60 | revolver clip; magazine prop | magazine modelled into the body ahead of the grip, slanted, charging handle | as the Prizrak, along the slant | as the Prizrak |
| `w033` CS9 | 36 | pistols clip; the Wraith's magazine prop | magazine in the grip, charging handle | as the Pozhar | the hand shows the Wraith's longer magazine, which the game drops for it too; the handle is a class default |
| `w111` U-39 Plechovka | 35 | rifle clip; moves `model.magazine` to the hand | magazine part, charging handle | knock the old magazine out with the fresh one, eject it, or pull it; push the fresh one up into the well ahead of the grip | |
| `w112` Predator | 30 | rifle clip | magazine part, charging handle | as the U-39 | |
| `w117` CSV | 40 | rifle-back clip | magazine part behind the grip, charging handle | as the U-39, into the well behind the grip | bullpup |
| `w118` UP-61 | 25 | rifle-back clip | magazine part behind the grip, charging handle | as the CSV | bullpup |
| `w119` Bavarium Splitter | 60 | rifle-back clip | magazine part behind the grip, charging handle | as the CSV | DLC; the handle is far forward on the right and may not be the charging handle |
| `w121` Bildt Negotiator | 3 | rifle-back clip; magazine prop | magazine part (a box of grenades), no action; launcher | eject or knock out the empty box, push a fresh one up into the well | launcher catch radius; unlike the one-round launchers, the empty box holds the well |
| `w123` UPU-210 | 1 | its own clip moves `model.cartridge_full` and `model.cartridge_empty` to the hand; both parts start hidden on the gun | magazine (the cartridge), no action; launcher | push a cartridge up into the bay under the receiver; a spent cartridge is replaced as the fresh one seats | the prop is not the cartridge part, so the hand draws the gun's own `model.cartridge_full` |
| `w131` Urga Vdova 89 | 80 | its own clip moves `model.magazine` to the hand; the ammunition belt spins and stays | magazine part beside the receiver, charging handle | as the U-39, into the box on the right of the receiver | the mouth is the receiver's underside over the box; the handle may be a latch |
| `w132` Capstone A-92 | 60 | default rifle clip; the Vdova's magazine prop | as the Vdova | as the Vdova | uses the Vdova's body and magazine |
| `w141` UVK-13 | 1 | rocket clip; the launcher shows `model.rpg` again once refilled | rocket, no action; launcher | push a rocket back into the front of the tube, rear end first | nothing to eject; the spent rocket is replaced |
| `w141_mm110` UVK-13 | 1 | as the UVK-13 | rocket | as the UVK-13 | its tuning has unlimited ammunition, so it is loaded again after each shot |
| `w142` Bloodhound | 1 | rocket clip | rocket | as the UVK-13 | |
| `w143` Fire Leach | 1 | rocket clip | rocket | as the UVK-13 | |
| `w144` Capstone Hydra | 1 | its own clip; the pod stays on the gun; the prop is the pod | magazine (the pod), no action; launcher | push a pod forward into the rear of the housing; a spent pod is replaced as the fresh one seats; a part-fired pod is ejected first | the hand draws the gun's own `model.rocket_pod` |
| `w146` Javelin | 1 | default rifle clip (no clip of its own) | rocket | as the UVK-13 | |
| `w704` M488 | 1 | rocket clip | rocket | as the UVK-13 | |
| `w176` Stupka 210 | 1 | mortar clip; moves no part | rocket (the shell into the muzzle) | push a shell into the muzzle | |
| `w151` U-Rys 96 | 6 | shotgun start, loop, and end; one round per loop (bullet reload) | shell through the loading port, pump | push shells one at a time up into the port | nothing is drawn in the hand; the pump is worked by hand only with the manual action on |
| `w153` Automat 12 | 14 | rifle-back clip; no magazine part and no prop | magazine modelled into the body behind the grip, no action | eject (nothing drops), push a magazine up into the base of the magazine | nothing is drawn in the hand, and the old magazine stays drawn |
| `w155` U-Rys 12 | 4 | its own clip; the gun's own act breaks it open; refills all four at once | shell into the breech, no action | push shells one at a time into the breech | the game loads all four in one step, the mod one per push; nothing is drawn in the hand |
| `w161` U-SV 45 | 4 | rifle clip | magazine part, bolt | as the U-39 | the bolt is the `ejection_port` part on the right of the receiver |
| `w162` CS111 Archangel | 6 | its own sniper clip | magazine part, bolt | as the U-39 | |
| `w163` Final Argument | 6 | its own sniper clip | magazine part, bolt | as the U-39 | the magazine enters from the side |

## Not reloaded by hand

These keep the game's reload, or never reload:

- `w013` Balloon Head: its ammunition kind is unlimited, so it never reloads.
- `w147` guitar case, `w701` missile strike (and its AI copy), `w702` EMP, and the power core (`w210_powercore`): untimed reloads, a timer with no clip. There is nothing to do with the hands, and the game's timer reloads them.
- `w173` Ragnarok: a special weapon with a magazine of ten, the Vdova's body without its magazine part, and no magazine prop. It has no spawn definition and no clip of its own; its class keeps the game's reload. Reloading it by hand would need a well on a magazine its model does not separate; the game's reload is the right fallback.
- `w177` Typhoon, `w178` balloon gun, `w179` Namaste, `w180` Lightning Gun, and `w703` Sungun: no magazine limit, so they never reload.
- The minigun (`w201_minigun`) and the .50 cal (`w206_mounted_capstone_m2hm`), carried after being torn off their mounts, and the mounted sniper rifle (`w205`): no magazine limit.
- The grenade, the beacon, and C4: thrown and planted, not reloaded.
- The stationary and vehicle weapons: not held.
- `w999_dlc_testgun`: a test shotgun the game never gives.

## Recommendations

The weapons reloaded by hand with nothing drawn in the hand (the revolvers, the U-Rys 96, the Automat 12, and the U-Rys 12) still reload: the round catches and seats where the well is. The game has no model for their rounds either. If an invisible round reads as broken in the headset, the shotguns could draw the game's shell casing model from their shell-ejection effect, and the Automat 12 could fall back to the game's reload, since its magazine is part of its body and can neither leave nor be seen to arrive.

## In the headset

Tick each weapon: draw it, fire it dry, reload it as the table says, and fire again.

1. Pozhar 98, Spectre Mark V: dual-wielded, each pistol on its own pouch, and once single.
2. Little General, Peacekeeper: eject, push a cylinder into the rear of the cylinder.
3. Prizrak U4, Wraith 2000, CS9: the magazine in the hand lines up with the gun's own as it seats.
4. U-39, Predator, U-SV 45, Archangel, Final Argument: knock the old magazine out with the fresh one.
5. CSV, UP-61, Bavarium Splitter: the well behind the grip; the Splitter's well is found at all.
6. Vdova 89, A-92: the box on the right of the receiver.
7. Bildt Negotiator: the empty box has to leave before the fresh one seats.
8. UPU-210, Hydra: a fresh round seats over a spent one in one push.
9. UVK-13, Bloodhound, Fire Leach, Javelin, M488, Stupka 210: the round into the muzzle.
10. U-Rys 96, U-Rys 12: shells one at a time, until the pouch stops offering them at full.
11. Automat 12: the invisible magazine seats; note whether it reads as broken.
12. Balloon Head, guitar case, Ragnarok, missile strike, EMP: the game's own reload (or none) runs, and the hands are not asked for anything.
