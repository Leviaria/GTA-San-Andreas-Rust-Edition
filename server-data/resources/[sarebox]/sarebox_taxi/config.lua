-- Taxi dispatch runs. There are no NPC passengers in SARE yet, so a run is
-- a drive to a dispatch point in your car, not passenger transport.
TaxiConfig = {}

-- Who may take runs: players with this SARE Box job, on duty.
TaxiConfig.Job = 'taxi'

-- Fare in whole dollars: Base + PerMeter * straight-line distance, at most Max.
TaxiConfig.Fare = { Base = 40, PerMeter = 0.12, Max = 450 }
-- Paid into this SARE Box account.
TaxiConfig.Account = 'cash'

-- Arrival: within Radius metres (map distance, ignoring height) and in a car.
TaxiConfig.Radius = 12.0
-- A dispatch point is at least MinDistance and at most MaxDistance away.
TaxiConfig.MinDistance = 250.0
TaxiConfig.MaxDistance = 3000.0
-- Faster than this average (m/s, about 180 km/h) is not paid.
TaxiConfig.MaxSpeed = 50.0
-- A run expires after this many seconds.
TaxiConfig.TimeLimit = 600
-- Seconds between finished runs, per character.
TaxiConfig.Cooldown = 20

-- Dispatch points in Los Santos (San Andreas world coordinates).
TaxiConfig.Points = {
  { label = 'Grove Street', x = 2495.0, y = -1687.0, z = 13.5 },
  { label = 'Ganton Gym', x = 2229.0, y = -1721.0, z = 13.5 },
  { label = 'Idlewood Pizza Stack', x = 2105.0, y = -1806.0, z = 13.5 },
  { label = 'Unity Station', x = 1743.0, y = -1861.0, z = 13.6 },
  { label = 'Pershing Square', x = 1481.0, y = -1740.0, z = 13.5 },
  { label = 'County General Hospital', x = 2034.0, y = -1405.0, z = 17.2 },
  { label = 'Jefferson Motel', x = 2229.0, y = -1159.0, z = 25.8 },
  { label = 'Verona Mall', x = 1130.0, y = -1450.0, z = 15.8 },
  { label = 'Market Station', x = 820.0, y = -1350.0, z = 13.5 },
  { label = 'Los Santos Airport', x = 1685.0, y = -2240.0, z = 13.5 },
  { label = 'East Los Santos', x = 2420.0, y = -1430.0, z = 24.0 },
}
