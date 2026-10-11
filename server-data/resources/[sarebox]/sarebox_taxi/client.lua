-- Taxi menu and directions. Everything that matters (where to go, the fare,
-- arrival and payment) is decided by the server; this only shows it.
local run = nil
local blip = nil

local directions = { 'north', 'north-east', 'east', 'south-east', 'south', 'south-west', 'west', 'north-west' }
local function direction(dx, dy)
  -- San Andreas: +y is north, +x is east.
  local angle = math.deg(math.atan(dx, dy)) % 360
  return directions[math.floor((angle + 22.5) / 45) % 8 + 1]
end

local function draw()
  if not run then return end
  local here = GetEntityCoords(PlayerPedId())
  local dx, dy = run.x - here.x, run.y - here.y
  local distance = math.sqrt(dx * dx + dy * dy)
  local where = distance <= run.radius and 'here: stop in your car'
    or ('%d m %s'):format(math.floor(distance), direction(dx, dy))
  UI.showTextUI(('Taxi: drive to %s\n%s  |  fare $%d'):format(run.label, where, run.fare))
end

-- The dispatch point on the radar and map, with the GPS route to it.
local function clearBlip()
  if blip then RemoveBlip(blip) end
  blip = nil
end

RegisterNetEvent('sarebox_taxi:client:route', function(data)
  run = data
  clearBlip()
  blip = AddBlipForCoord(run.x, run.y, run.z)
  SetBlipSprite(blip, 198)
  SetBlipColour(blip, 5)
  SetBlipRoute(blip, true)
  BeginTextCommandSetBlipName('STRING')
  AddTextComponentString('Taxi: ' .. run.label)
  EndTextCommandSetBlipName(blip)
  draw()
end)

RegisterNetEvent('sarebox_taxi:client:clear', function()
  run = nil
  clearBlip()
  UI.hideTextUI()
end)

CreateThread(function()
  while true do
    Wait(500)
    draw()
  end
end)

RegisterCommand('taxi', function()
  local options = {}
  if run then
    options[#options + 1] = { title = 'Cancel dispatch', description = ('Going to %s, $%d'):format(run.label, run.fare),
      serverEvent = 'sarebox_taxi:server:cancel' }
  else
    options[#options + 1] = { title = 'Take a dispatch', description = 'Be in a car. Drive to the point you get to earn the fare.',
      serverEvent = 'sarebox_taxi:server:start' }
  end
  options[#options + 1] = { title = 'Sign up as a taxi driver', description = 'For unemployed players',
    serverEvent = 'sarebox_taxi:server:signup' }
  UI.registerContext({ id = 'sarebox_taxi', title = 'Taxi dispatch', options = options })
  UI.showContext('sarebox_taxi')
end)
RegisterKeyMapping('taxi', 'Taxi dispatch', 'keyboard', 'J')

AddEventHandler('onClientResourceStart', function(resource)
  if resource == GetCurrentResourceName() then TriggerServerEvent('sarebox_taxi:server:status') end
end)
