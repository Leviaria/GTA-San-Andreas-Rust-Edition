-- Admin commands. They are restricted: a player needs `command.<name>` in the
-- ACL, which server.cfg gives group.admin. Make yourself admin with
--   add_principal identifier.license:<your license> group.admin
-- (type /id in game to see your license).
local Core = exports.sarebox:GetCoreObject()

local function reply(src, text)
  if src == 0 then
    print(text)
  else
    Core.Functions.Notify(src, text, 'inform', 'Admin')
  end
end

local function target(src, value)
  local id = tonumber(value)
  local player = id and Core.Functions.GetPlayer(id)
  if not player then reply(src, ('No player with server ID %s'):format(tostring(value))) end
  return player, id
end

local function isAdmin(src)
  return src == 0 or IsPlayerAceAllowed(src, 'command.admin')
end

local actions = {}

function actions.givemoney(src, id, account, amount)
  local player = target(src, id)
  if not player then return end
  if player.Functions.AddMoney(account, amount, 'admin') then
    reply(src, ('Gave %s $%s %s'):format(player.PlayerData.name, amount, account))
  else
    reply(src, 'Usage: /givemoney <id> <cash|bank> <amount>')
  end
end

function actions.setmoney(src, id, account, amount)
  local player = target(src, id)
  if not player then return end
  if player.Functions.SetMoney(account, amount, 'admin') then
    reply(src, ('Set %s %s to $%s'):format(player.PlayerData.name, account, amount))
  else
    reply(src, 'Usage: /setmoney <id> <cash|bank> <amount>')
  end
end

function actions.setjob(src, id, job, grade)
  local player = target(src, id)
  if not player then return end
  if player.Functions.SetJob(job, tonumber(grade) or 0) then
    reply(src, ('%s is now %s'):format(player.PlayerData.name, player.PlayerData.job.label))
  else
    local names = {}
    for name in pairs(Core.Config.Jobs) do names[#names + 1] = name end
    table.sort(names)
    reply(src, 'Jobs: ' .. table.concat(names, ', '))
  end
end

actions['goto'] = function(src, id)
  local _, targetId = target(src, id)
  if not targetId or src == 0 then return end
  local coords = GetEntityCoords(GetPlayerPed(targetId))
  SetEntityCoords(GetPlayerPed(src), coords.x + 1.0, coords.y, coords.z)
end

function actions.bring(src, id)
  local _, targetId = target(src, id)
  if not targetId or src == 0 then return end
  local coords = GetEntityCoords(GetPlayerPed(src))
  SetEntityCoords(GetPlayerPed(targetId), coords.x + 1.0, coords.y, coords.z)
end

function actions.kick(src, id, ...)
  local player, targetId = target(src, id)
  if not player then return end
  local reason = table.concat({ ... }, ' ')
  DropPlayer(targetId, reason ~= '' and reason or 'Kicked by an admin')
  reply(src, ('Kicked %s'):format(player.PlayerData.name))
end

function actions.announce(_, ...)
  local text = table.concat({ ... }, ' ')
  if text == '' then return end
  TriggerClientEvent('sare:ui:notify', -1, { title = 'Announcement', description = text, type = 'warning', duration = 8000 })
end

-- Every admin action is logged on the server console with who did it.
local function run(src, name, args)
  local who = src == 0 and 'console' or ('%s (%d, %s)'):format(GetPlayerName(src), src,
    GetPlayerIdentifierByType(src, 'license') or 'no license')
  print(('[admin] %s: %s %s'):format(who, name, table.concat(args, ' ')))
  actions[name](src, table.unpack(args))
end

for name in pairs(actions) do
  RegisterCommand(name, function(src, args) run(src, name, args) end, true)
end

-- The admin menu (F11 or /admin): the client asks, the server checks again
-- before every action.
RegisterCommand('admin', function(src)
  if src == 0 then return end
  local list = {}
  for _, id in ipairs(Core.Functions.GetPlayers()) do
    local data = Core.Functions.GetPlayer(id).PlayerData
    list[#list + 1] = {
      id = id,
      name = data.name,
      job = data.job.label,
      cash = data.money.cash,
      bank = data.money.bank,
    }
  end
  TriggerClientEvent('sarebox_admin:menu', src, list)
end, true)

RegisterNetEvent('sarebox_admin:action', function(name, args)
  local src = source
  if not isAdmin(src) or not actions[name] or not IsPlayerAceAllowed(src, 'command.' .. name) then
    Core.Functions.Notify(src, 'Access denied', 'error', 'Admin')
    return
  end
  if type(args) ~= 'table' then args = {} end
  local clean = {}
  for i = 1, math.min(#args, 8) do clean[i] = tostring(args[i]):sub(1, 256) end
  run(src, name, clean)
end)
