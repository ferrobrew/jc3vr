# Feedback from the 10 November 2026 11:32 session

- the dev ui started visible in headset - that should not persist / be on by default
- I couldn't interact with the dev UI until I unfocused/focused the window. two issues here: a) we seem to be convinced that the game is unfocused when it is, at least initially. state initialisation issue? b) the window focus doesn't matter for dev-menu-in-world use, which is driven entirely by virtual cursor. it only matters when it's being used in non-VR (e.g. with the game window itself)
- we should 1.5x the size of the holsters so that we can be looser with putting guns back in the holster (you can see them drop at ~39.5s - that's something that should have gone in)
- we still can't grapple-snatch weapons off the ground. this was a regression introduced within the last few days. picking them up by hand works fine
- the camera seems to closely follow animations while walking around, which results in a bobbing motion. no good. need to stop that
- I keep forgetting to mention this, but I'm pretty sure I can't use my left hand to pick things up from my back?
- 138.3s: the weapon snaps to my left hand when I apply the grab. undesirable. it should either slide towards it, or not let me grab it if it's this far off; leaning towards the latter, with a slide when it's reasonably close
- 157s: we need feedback to indicate that the weapon's already had ammo loaded and won't accept any more. make the hint red, play a reject sound of some kind if you try to insert it?
- 176s: my head is noticeably detached from my body here. stunt position quirk? recentering helps but I had to get off the plane for it to realign properly
- 193s: let's make the door grab target larger - let's try 2x - so that it's more generous for grabs like this
- we should hide the C4 detonate button/hint when you don't have any C4 out in the world
