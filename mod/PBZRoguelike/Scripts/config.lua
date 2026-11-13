-- Settings of the in-game bridge. Edit, then restart the game (or reload mods in UE4SS).
local Config = {}

-- Where the bridge and the director exchange files. Leave nil: the bridge then uses the `Runtime`
-- folder next to `Scripts`. Set an absolute path with forward slashes only if the log says the
-- folder cannot be written, for example:
--   Config.RuntimePath = "C:/Program Files (x86)/Steam/steamapps/common/PBZ/platform/steam/ue4ss/Mods/PBZRoguelike/Runtime"
Config.RuntimePath = nil

-- Keys, by the engine's key names (One, Two, ... NumPadOne, F1 ... F12, Q, E, ...).
-- The choice keys only count while the panel is on screen.
Config.ChoiceKeys = { 'One', 'Two', 'Three', 'Four', 'Five', 'Six', 'Seven', 'Eight', 'Nine' }
Config.ToggleKey = 'F6'

-- The panel.
Config.PanelX = 48
Config.PanelY = 96
Config.PanelWidth = 760
Config.PanelFontSize = 16
Config.PanelOpacity = 0.72
Config.PanelZOrder = 40

-- Where enemies appear: this far in front of you (centimetres), this far apart, this far above the ground.
Config.SummonDistance = 600
Config.SummonSpacing = 250
Config.SummonHeight = 50

-- How often keys are read and how often the bridge talks to the director, in milliseconds.
Config.KeyIntervalMs = 16
Config.BridgeIntervalMs = 100

return Config
