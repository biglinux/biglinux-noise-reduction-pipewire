-- BigLinux Microphone
--
-- Keep libpipewire-module-echo-cancel's capture stream connected to the
-- currently selected physical microphone. The public smart filter
-- (`mic-biglinux`) must not be considered as an upstream target here,
-- otherwise the AEC captures the already-filtered virtual mic and the
-- echo path is never subtracted.

lutils = require ("linking-utils")
log = Log.open_topic ("s-biglinux-aec")

local EC_CAPTURE_NODE_NAME = "echo-cancel-capture"
local EC_SOURCE_NAME = "echo-cancel-source"

local VIRTUAL_SOURCE_NAMES = {
  ["mic-biglinux"] = true,
  ["mic-biglinux-capture"] = true,
  ["echo-cancel-source"] = true,
  ["echo-cancel-capture"] = true,
  ["echo-cancel-reference"] = true,
  ["output-biglinux"] = true,
}

local function starts_with (text, prefix)
  return text ~= nil and text:sub (1, #prefix) == prefix
end

local function json_name (metadata, key)
  if metadata == nil then
    return nil
  end

  local value = metadata:find (0, key)
  if value == nil then
    return nil
  end

  local ok, parsed = pcall (function ()
    return Json.Raw (value):parse ()
  end)
  if ok and parsed then
    return parsed ["name"]
  end
  return nil
end

local function metadata_object (source, name)
  local metadata_om = source:call ("get-object-manager", "metadata")
  return metadata_om:lookup {
    Constraint { "metadata.name", "=", name }
  }
end

local function disable_legacy_aec_smart_filter (source, si)
  local node = si:get_associated_proxy ("node")
  if node == nil or node.properties ["node.name"] ~= EC_SOURCE_NAME then
    return
  end

  -- Older configurator builds wrote `filter.smart = true` on
  -- `echo-cancel-source`. WirePlumber then reinserted the AEC source
  -- into the smart-filter sorting pass and could rebuild the bad loop:
  -- `echo-cancel-capture <- mic-biglinux`. The "filters" metadata
  -- overrides node properties, so force this internal AEC source to be
  -- a plain virtual source even if an old config file is still present.
  local metadata = metadata_object (source, "filters")
  local id = node ["bound-id"]
  if metadata ~= nil and id ~= nil then
    metadata:set (id, "filter.smart", "Spa:String:JSON", "false")
  end
end

local function is_virtual_source_name (name)
  return VIRTUAL_SOURCE_NAMES [name]
      or starts_with (name, "input.pw-loopback")
      or starts_with (name, "output.pw-loopback")
      or starts_with (name, "input.loopback")
      or starts_with (name, "output.loopback")
end

local function is_real_capture_source (si)
  local props = si.properties
  local name = props ["node.name"]
  local media_class = props ["media.class"]

  if name == nil or name == "" then
    return false
  end
  if is_virtual_source_name (name) then
    return false
  end
  if media_class ~= "Audio/Source" and media_class ~= "Audio/Duplex" then
    return false
  end
  if props ["item.node.direction"] ~= "output" then
    return false
  end
  if props ["node.virtual"] == "true" or props ["node.link-group"] ~= nil then
    return false
  end
  if starts_with (name, "alsa_output.") or name:find ("%.monitor$") then
    return false
  end

  return true
end

local function lookup_real_source_by_name (om, name)
  if name == nil then
    return nil
  end

  for si in om:iterate { type = "SiLinkable" } do
    if si.properties ["node.name"] == name and is_real_capture_source (si) then
      return si
    end
  end
  return nil
end

local function source_priority (si)
  local props = si.properties
  return tonumber (props ["priority.session"])
      or tonumber (props ["priority.driver"])
      or tonumber (props ["node.id"])
      or 0
end

local function best_real_source (om)
  local best = nil
  local best_priority = -1

  for si in om:iterate { type = "SiLinkable" } do
    if is_real_capture_source (si) then
      local priority = source_priority (si)
      if best == nil or priority > best_priority then
        best = si
        best_priority = priority
      end
    end
  end

  return best
end

local function selected_real_source (source, om)
  local metadata = metadata_object (source, "default")

  -- `wpctl set-default` writes default.configured.audio.source; prefer
  -- that so the selected physical mic wins even if another virtual
  -- source has a higher session priority.
  local configured_name = json_name (metadata, "default.configured.audio.source")
  local target = lookup_real_source_by_name (om, configured_name)
  if target then
    return target
  end

  local default_name = json_name (metadata, "default.audio.source")
  target = lookup_real_source_by_name (om, default_name)
  if target then
    return target
  end

  return best_real_source (om)
end

SimpleEventHook {
  name = "biglinux/echo-cancel-source-not-smart",
  before = "linking/rescan-trigger",
  interests = {
    EventInterest {
      Constraint { "event.type", "=", "session-item-added" },
      Constraint { "event.session-item.interface", "=", "linkable" },
    },
  },
  execute = function (event)
    disable_legacy_aec_smart_filter (event:get_source (), event:get_subject ())
  end
}:register ()

SimpleEventHook {
  name = "biglinux/echo-cancel-capture-target",
  after = "linking/find-defined-target",
  before = {
    "linking/find-filter-target",
    "linking/find-default-target",
    "linking/find-best-target",
    "linking/prepare-link",
  },
  interests = {
    EventInterest {
      Constraint { "event.type", "=", "select-target" },
    },
  },
  execute = function (event)
    local source, om, si, si_props, si_flags, target =
        lutils:unwrap_select_target_event (event)

    if target or si_props ["node.name"] ~= EC_CAPTURE_NODE_NAME then
      return
    end

    target = selected_real_source (source, om)
    if not target then
      log:warning (si, "no physical source available for echo cancel capture")
      event:stop_processing ()
      return
    end

    if not lutils.canLink (si_props, target) then
      log:warning (si, "selected source is not linkable for echo cancel capture")
      event:stop_processing ()
      return
    end

    local passthrough_compatible, can_passthrough =
        lutils.checkPassthroughCompatibility (si, target)
    if not passthrough_compatible then
      log:warning (si, "selected source passthrough is incompatible")
      event:stop_processing ()
      return
    end

    -- Prevent `linking/get-filter-from-target` from replacing this
    -- physical source with the targetless `mic-biglinux` smart filter.
    si_flags.has_defined_target = true
    si_flags.has_node_defined_target = false
    si_flags.can_passthrough = can_passthrough
    event:set_data ("target", target)
    log:info (si,
      "routing echo cancel capture to "
      .. tostring (target.properties ["node.name"]))
  end
}:register ()
