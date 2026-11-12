# Feedback from the 14:18 session

- I still couldn't grapple-snatch things off the ground, but I could pick them up manually just fine. this is a regression introduced over the last two days
- we seem to be hiding the virtual yoke/throttle now. we shouldn't do that - even if we have a real yoke to use, the virtual one is still useful to understand exactly what your input's doing
- the last helicopter I was in, the throttle was outside the helicopter - it should be brought much closer in
- we should show the full range of motion for the throttle to make it obvious what's possible, especially in planes
- I couldn't shoot the minigun in the helicopter I was in. I think this is because it's tied to head aim, and our setup breaks that. let's investigate to see what we can do there: can we make aiming work, or should we just have it lock forward instead?
- I should be able to twist the stick in helicopters/planes to yaw (rudder)
- sometimes, when I pick up a two-handed weapon, my left-hand holster's weapon appears in my left hand, despite me not grabbing it
- ragdoll-grabbing people still doesn't work at all
- when I do grab a ragdoll successfully, I lose grip very soon after; they do not follow my hands. ideally, I would be able to pick up a ragdoll (alive or dead) and toss it over a cliff
- c4 trigger is still on the wrong facing direction of the hand. flip it around so that it is always triggered when approached from the top (e.g. palm facing away from you)
- minigun use did not work at all. could not grab the minigun externally, and entering the minigun use mode put me in a hard-to-escape state where I couldn't do anything, including using the minigun. as with the mounted MG before (same code, hopefully!), we want this to be naturak and in-world: grip the machine/minigun to use, shoot immediately, pull hard enough to detach
- grapple-swinging/suspending from a point didn't seem to work so good this time. we struggled to actually stay attached, coming off with ease, and detaching mid-swing did not preserve exit velocity. it also seems slow to respond: it doesn't convert to a fixed swing-rope for a few hundred milliseconds, possibly a second or more
- stunt position is completely broken. we're in a broken crouching pose and our arms lag well behind where they're meant to be. probably not accounting for the movement? either way, can't do anything interesting while stunting
- can't transition into wingsuit while grappling by stretching arms. also can't do the same while parachuting. both of these should be possible
