# Feedback

1. Update the grid to create a grid visible only to the player, similar to StarCraft II. In StarCraft II, units move on the true coordinate grid (X, Y, Z), while a larger visible grid is used for building placement. Implement the same system: a visible grid for all players to place buildings, and a true coordinate grid for unit movement. The smallest building should occupy exactly one square of the visible grid, and the barracks should occupy a 2 × 2 square. Ensure the buildings align with the grid.

2. When you use the worker and click on a resource to collect, or any other unit to go to, show a circle around that unit to signify that it is the target for the unit currently moving toward it. Even if you click on another unit or unselect the unit, when you select the unit again and the target is the same, always show the target where the unit is moving towards with a green circle.
   When you select a worker and click on a mineral field or a gas geyser, its status updates to “moving to mineral” or “moving to gas.” Then, when the worker is actually on the mineral or gas, the status changes to “collecting mineral” or “collecting gas.” Finally, when it returns the resources to the command center, the status is “returning mineral” or “returning gas.”

3. When clicking on a building, a mineral field, a gas station, or any entity in the game, it should show the unit as selected, displaying its stats with a green circle around it.

4. I encountered a bug where a worker building a barrack is blocked because the barrack spawned over it, and I cannot move the worker anymore. Make sure that whenever a barrack or any building is built by a worker, at the end the worker has a phasing phase where it can phase out of the building and then regains collision, to ensure it can exit the building it is currently constructing.

5. Whenever you click somewhere that is not an entity but just a place on the map, show, like in any RTS, an animation and put a flag there that indicates the unit is moving in that direction. Also add a dashed line or arrow to show the direction and the remaining pathfinding of the unit.

6. For any building that can produce units, add a rally flag and a rally point. You can place the rally point whenever the building is selected by right‑clicking on the map. Whenever a unit is produced, it automatically receives the command to move to that location. You can click on other entities, and the unit will automatically follow them, moving toward the entity and following it.

7. Add a scenario with all already buildings built, enemy units going towards the base and soldiers. That scenario must not be timed, it must be for manual testing.
