# Native Freeroam Roadmap

Next milestone: **drive together with friends in your own cars**, with up to
20 players including the host. Missions are not a priority. This document
describes planned work; [multiplayer status](multiplayer.md) and
[freeroam status](freeroam-status.md) describe what is currently implemented.

## Priorities

| Priority | Work | Milestone criteria |
| --- | --- | --- |
| 1 | Multiplayer across different PCs and networks | Two separate PCs can connect, drive between streamed areas, disconnect and reconnect. Test both LAN and internet connections; testing on a single PC does not cover this. |
| 2 | Shared cars and passengers — initial version implemented | Personal cars remain visible after the driver exits, and the host reserves three passenger seats. Switching cars, model-specific seat counts and shared collision response remain to be implemented. |
| 3 | Cars, peds, outfits and resource versions — initial version implemented | Selected models and clothing choices are shared from a validated resource list. Continue testing across different PCs with larger mod packs. |
| 4 | Audio — engines and local footsteps implemented | Original engine loops respond to speed and position, including in multiplayer. Local footsteps follow grounded movement. Collision sounds, material-specific sounds and remote players' footsteps remain to be implemented. |
| 5 | Traffic and pedestrian NPCs | Start in one neighborhood with a limited number of actors; measure performance and define who simulates them in multiplayer. |
| 6 | Local offline saves — initial version implemented | A safe outdoor position, camera, car, ped and outfit are restored. Interiors, server progress, continuous autosaving and exact vehicle parking positions remain to be implemented. |
| 7 | Day/night cycle, weather and Classic/Enhanced profiles | Consistent lighting and weather with selectable visual styles, and synchronized time and weather for everyone in a session. The current graphics presets are not a complete day/night or weather simulation. |

Streaming, performance and vehicle physics still need tuning. Suspension,
pitch/roll and axle-based tire forces are implemented; animated wheels, damage
and full physical collision response remain to be implemented. A capacity test
with 20 network clients does not demonstrate performance with 20 fully
rendering player clients.

## Player hosting with a server browser and join codes

The current host simulates its local world and relays player poses.
An initial relay implementation with a server browser and public/private rooms
is now available; see the [setup guide](multiplayer.md). `sa-relay` registers
the server name, player count and runtime version. Private rooms are omitted
from the list and use a random join code as an invitation. Both sides establish
outbound connections, and all traffic passes through the relay.
Resource lists, version checks and automatic downloading/caching are now integrated.

Direct P2P without manual port forwarding requires NAT traversal and a relay
fallback when a direct connection fails, including behind CGNAT. Direct P2P
remains future work. The host is still a player's PC when traffic passes
through the relay; a dedicated game server application is not required.
The directory and relay do, however, need a reachable service with hosting
and configuration. No such public service is currently set up for the project.

Possible service-based solutions include
[Epic Online Services P2P/lobbies](https://dev.epicgames.com/docs/epic-online-services/multiplayer/nat-p2p-interface/eosp-2-p-sample)
or [Steam Networking](https://partner.steamgames.com/doc/features/multiplayer/networking).
The choice and project setup must be settled before integration; this
documentation does not mean the project already has access to their production
services. Direct IP remains an option for local testing.

## Automatic mod downloads and caching

The initial joining flow is implemented:

1. Fetch the host's ordered resource list: resource ID, format version,
   filename, size and SHA-256 for each file. Check runtime compatibility.
2. Show required resources, download size and progress. The user's automatic
   download preference determines whether new files are fetched without an
   additional prompt.
3. Reuse files in a separate cache when their content hashes match. Fetch only
   missing or changed files, with limits on file size and total storage.
4. Download to temporary files and verify their actual size and hash before
   using them. Interrupted or invalid files are never activated as complete
   resources.
5. Load a separate resource set for the session in the agreed order. Local
   mods are not overwritten. Restore the local resource set on disconnect.

The cache should identify content, not just server names. A changed resource
gets a new hash, while unchanged files can be reused on the next join. A hash
detects changed or corrupted files; it does not make the host or the file
contents trustworthy. Downloads should allow supported data resources while
rejecting absolute paths, `..`, links outside the cache directory and
executable plugins/scripts. A cache overview showing disk usage and allowing
unused resources to be deleted remains to be implemented.

The initial scope covers native manifests, DFF/TXD/PNG/COL/IFP and supported
cars, peds, clothing and placements. GTA V/FiveM models must be converted on
the host side before they can be shared as native resources; automatic
downloading does not make Lua scripts or arbitrary FiveM packages compatible.
Original San Andreas files must still be read from each player's own
installation. The host must only share mods they have the right to distribute.

The loader now prepares a separate resource set on a background thread and
uploads GPU data incrementally before switching the complete set into use.
The offline world is preserved and restored on disconnect. Tests cover cache
hits, version changes, incorrect hashes, interruptions, invalid paths and
resource switching/restoration in two Vulkan instances. The selected car,
ped and outfit are now synchronized, with separate clothing rendering for
each player.

## Server model inspired by ReSkate

Hosting for friends remains available in the game, and `sa-server` can now run
without a game window or an original installation. See the
[server setup guide](server-hosting.md). Both can register in the same browser
and use join codes and the native resource cache. The initial transport is
still the project's own relay. Steam lobbies and Valve's relay require
separate integration work and an agreed Steamworks setup.

Reference: [ReSkate's server setup](https://github.com/Dingo-Shenanigans/ReSkate/blob/main/Server/README.txt),
checked on October 7, 2026. Its Steam integration has not been added to this project.
