-- Taxi dispatch runs. The server owns every run: it picks the dispatch
-- point, computes the fare, watches the player's position and pays once.
-- The game only asks to start or cancel a run; it never names a place, an
-- amount or the moment of arrival.
local Core = exports.sarebox:GetCoreObject()
local Config = TaxiConfig

local Runs = {}     -- [server id] = the player's open run
local Finished = {} -- [citizen id] = GetGameTimer() of the last paid run

local function notify(src, text, kind)
  Core.Functions.Notify(src, text, kind or 'inform', 'Taxi')
end

local function flat(a, b)
  local dx, dy = a.x - b.x, a.y - b.y
  return math.sqrt(dx * dx + dy * dy)
end

local function onDuty(player)
  local job = player and player.PlayerData.job
  return job ~= nil and job.name == Config.Job and job.onduty == true
end

local function fare(distance)
  return math.min(Config.Fare.Max, math.floor(Config.Fare.Base + distance * Config.Fare.PerMeter))
end

local function route(src, run)
  local point = Config.Points[run.point]
  TriggerClientEvent('sarebox_taxi:client:route', src, {
    label = point.label, x = point.x, y = point.y, z = point.z,
    fare = run.fare, radius = Config.Radius,
  })
end

-- Close a run without paying.
local function cancel(src, text, kind)
  if not Runs[src] then return end
  Runs[src] = nil
  TriggerClientEvent('sarebox_taxi:client:clear', src)
  if text then notify(src, text, kind) end
end

RegisterNetEvent('sarebox_taxi:server:start', function()
  local src = source
  local player = Core.Functions.GetPlayer(src)
  if not player then return end
  if Runs[src] then
    route(src, Runs[src])
    notify(src, 'You already have a dispatch. Finish or cancel it first.')
    return
  end
  if not player.PlayerData.license then
    return notify(src, 'Taxi runs need a saved character (update the game).', 'error')
  end
  if not onDuty(player) then
    return notify(src, 'Only on-duty taxi drivers get dispatches. Sign up in the taxi menu.', 'error')
  end
  local ped = GetPlayerPed(src)
  if not IsPedInAnyVehicle(ped, false) then
    return notify(src, 'Get in a car first.', 'error')
  end
  local last = Finished[player.PlayerData.citizenid]
  if last and GetGameTimer() - last < Config.Cooldown * 1000 then
    return notify(src, 'Dispatch has nothing for you yet. Try again in a moment.')
  end
  local here = GetEntityCoords(ped)
  local choices = {}
  for index, point in ipairs(Config.Points) do
    local distance = flat(here, point)
    if distance >= Config.MinDistance and distance <= Config.MaxDistance then
      choices[#choices + 1] = { index = index, distance = distance }
    end
  end
  if #choices == 0 then
    return notify(src, 'No dispatch point in range. Drive into Los Santos first.', 'error')
  end
  local pick = choices[math.random(#choices)]
  local run = {
    citizenid = player.PlayerData.citizenid,
    point = pick.index,
    distance = pick.distance,
    fare = fare(pick.distance),
    started = GetGameTimer(),
    last = { x = here.x, y = here.y, at = GetGameTimer() },
  }
  Runs[src] = run
  route(src, run)
  notify(src, ('Dispatch: drive to %s for $%d.'):format(Config.Points[run.point].label, run.fare), 'success')
end)

RegisterNetEvent('sarebox_taxi:server:cancel', function()
  cancel(source, 'Dispatch cancelled. No fare.')
end)

-- The game asks again after its script (re)starts.
RegisterNetEvent('sarebox_taxi:server:status', function()
  local src = source
  if Runs[src] then route(src, Runs[src]) end
end)

-- Unemployed players can sign up; nobody can switch away from another job
-- here, and the job is set by SARE Box on the server.
RegisterNetEvent('sarebox_taxi:server:signup', function()
  local src = source
  local player = Core.Functions.GetPlayer(src)
  if not player then return end
  local job = player.PlayerData.job.name
  if job == Config.Job then return notify(src, 'You already drive a taxi.') end
  if job ~= 'unemployed' then return notify(src, 'Quit your current job first.', 'error') end
  if player.Functions.SetJob(Config.Job, 0) then
    notify(src, 'You are a taxi driver now. Get in a car and take a dispatch.', 'success')
  end
end)

local function check(src, run)
  local player = Core.Functions.GetPlayer(src)
  if not player or player.PlayerData.citizenid ~= run.citizenid then
    Runs[src] = nil
    return
  end
  if not onDuty(player) then
    return cancel(src, 'You are no longer an on-duty taxi driver. Dispatch cancelled.', 'warning')
  end
  local now = GetGameTimer()
  if now - run.started > Config.TimeLimit * 1000 then
    return cancel(src, 'The dispatch expired.', 'warning')
  end
  local ped = GetPlayerPed(src)
  local here = GetEntityCoords(ped)
  -- A jump no car can make (a teleport) ends the run unpaid.
  local seconds = math.max(0.001, (now - run.last.at) / 1000)
  if flat(here, run.last) > Config.MaxSpeed * 2 * seconds + 30.0 then
    print(('[sarebox_taxi] %s (%d) moved %.0f m in %.1f s; run cancelled'):format(GetPlayerName(src), src, flat(here, run.last), seconds))
    return cancel(src, 'Dispatch cancelled: your position jumped.', 'error')
  end
  run.last = { x = here.x, y = here.y, at = now }
  local point = Config.Points[run.point]
  if flat(here, point) > Config.Radius or math.abs(here.z - point.z) > 15.0 then return end
  if not IsPedInAnyVehicle(ped, false) then return end
  local elapsed = (now - run.started) / 1000
  if elapsed < run.distance / Config.MaxSpeed then
    print(('[sarebox_taxi] %s (%d) arrived after %.1f s for %.0f m; not paid'):format(GetPlayerName(src), src, elapsed, run.distance))
    return cancel(src, 'Dispatch cancelled: that was impossibly fast.', 'error')
  end
  -- Close the run before paying, so it can only ever be paid once.
  Runs[src] = nil
  TriggerClientEvent('sarebox_taxi:client:clear', src)
  Finished[run.citizenid] = now
  if player.Functions.AddMoney(Config.Account, run.fare, 'taxi run') then
    notify(src, ('Arrived at %s. Paid $%d.'):format(point.label, run.fare), 'success')
  else
    notify(src, 'Arrived, but the payment failed. Tell an admin.', 'error')
  end
end

-- The server, not the game, decides when a car has arrived.
CreateThread(function()
  while true do
    Wait(500)
    for src, run in pairs(Runs) do check(src, run) end
  end
end)

AddEventHandler('playerDropped', function()
  -- A run ends with the connection; nothing is paid for it.
  Runs[source] = nil
end)

AddEventHandler('sarebox:server:onJobUpdate', function(src)
  local player = Core.Functions.GetPlayer(src)
  if Runs[src] and not onDuty(player) then
    cancel(src, 'Your job changed. Dispatch cancelled.', 'warning')
  end
end)

AddEventHandler('onResourceStop', function(resource)
  if resource ~= GetCurrentResourceName() then return end
  -- Runs live in memory only: a restart cancels them, unpaid.
  for src in pairs(Runs) do
    notify(src, 'Taxi dispatch restarted. Your run was cancelled; take a new one.', 'warning')
  end
end)
