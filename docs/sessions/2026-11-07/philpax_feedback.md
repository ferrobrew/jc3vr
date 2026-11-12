- we should cancel out the grappling animation when grapple-hanging (holding down the grapple button mid-grapple), so that our legs are free to swing as per normal
- moving my grapple arm during a grapple-hang should change my body velocity accordingly
- my launch velocity into a grapple-hang does not match the velocity I had while entering it - my velocity seems to be reset
- we don't handle body yaw correctly while parachuting - bending my body (as judged by our IK) should bend my body without necessarily changing our direction of motion. right now, the body's forward is fixed to the direction of movement, which is extremely weird when you yaw to shoot and your body doesn't bend to adjust
- object grapple-snatching seems to be at a fixed point on the object (its CoM)? that's not what we want. you should be able to grapple-snatch/swing any point of the object and have that be the attachment point
- the yoke in helicopters/planes doesn't do anything. is this split out in the model? is there anything we can do here?
- swimming still feels like it isn't really working. what stroke do I have to be doing to move?
- let's disable the line to the grapple target (but keep the grapple target) for a more minimalistic presentation
- there is no way for me to actually throw the rebel drop flare. that should be a projectile like the others
- can't seem to grab people to ragdoll them / grab them by where I held them
- similarly, limited ability to grab ragdolled corpses
- Rico seems a bit weak in general? my hands should be able to hold more weight than they should - I should not be struggling to carry a car door with two hands

also: we rely on a cut path for grappling in-vehicle, and it doesn't work; it doesn't go through any of the same logic as retail grappling. let's investigate fixing that, too, by replicating the retail grappling path, just in a car. this includes our additional features, too:
- grappling something should act as though we shot a permanent grapple from our car to the target, until released
- grappling A to B should work as per normal
- grapple-holding to a target should hold a grapple to that target until I let go, or grapple to another target
- grapple-snatching should work. I should be able to steal a gun from a NPC I'm driving past
