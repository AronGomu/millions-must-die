# Feedback

1. When I have a worker or any unit selected and I try to click on a crystal or a gas resource, the worker never starts to collect the resource. This means there is a discrepancy between the headless test and manually clicking in the game. Fix it. Make sure that if I click anywhere on the entire square that is the resource, the worker starts immediately gathering, moves, and collects the resource.

2. When I click on a unit, I should be able to click anywhere on the body of the sprite to select the unit, and click anywhere on the circle of the hitbox. Ensure there are two distinct types of hitbox: the player selection hitbox that matches closely with the sprite, and the collision hitbox between units that prevents them from entering each other.

3. Reduce the radius of the units' circular collision hitbox, and ensure that units cannot ever merge. Currently, units can merge together inside the circular collision hitbox; they should not be able to.

4. At the top right, add a settings icon. When clicked, it opens a menu referencing any RTS game. For now, the menu contains a single button: Settings. Clicking the Settings button opens another window. In that window, include options to update the scrolling speed and camera panning speed when the cursor moves to the edge of the screen. Keep the same style for now.

5. Also, in the settings, add an option to keep the mouse pointer inside the game, and create settings to allow the game to run fullscreen, fullscreen window, and just windowed. By default, the game should be in windowed fullscreen, and the mouse pointer option should keep the mouse within the screen.

6. Triple the speed of the units.

7. Update the bottom HUD to contain, on the left, a minimap and, on the right, a grid-like menu where you can select and see units—basically the same HUD as StarCraft, with a grid-like menu for building stuff with a worker. At the center, display the unit selection visualization. If only one unit is selected, show the details of that specific unit. On the left, show the map of the card; for now, do not implement detailed functionality. Add to the game a frontier: a maximum map size where you cannot pan the camera any further, and add a test to ensure the camera cannot exceed that limit. Also, as in most RTS games, ensure there is the maximum map size itself, the map itself, and the area given to the map where the camera can move. Those are two distinct things. And make sure that on the HUD, when I click on the map, it moves the camera exactly like in any RTS, and I can select buildings and place stuff on the grid menu for the workers.

8. Add background music and a single voice line for every unit when you click, select, and move them, exactly like any RTS. For now, the sound can just be a beep. And for the music, ask me to provide the music format and the music file. For the music file, use Terran One of Starcraft soundtrack. Make sure that the sounds are correctly adjusted volume-wise. And in the settings, add options to change the updates, the volume for the voice lines, and the music. There should be a master volume and a separate volume for the voice lines and the music itself.
