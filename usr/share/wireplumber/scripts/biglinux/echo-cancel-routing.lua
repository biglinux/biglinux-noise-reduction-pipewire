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
local EC_SINK_NODE_NAME = "echo-cancel-sink"
local JAMESDSP_SINK_NAME = "jamesdsp_sink"
local OUTPUT_FILTER_NODE_NAME = "output-biglinux"

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

local function lookup_sink_by_name (om, name)
  if name == nil then
    return nil
  end

  for si in om:iterate { type = "SiLinkable" } do
    local props = si.properties
    if props ["node.name"] == name
        and props ["media.class"] == "Audio/Sink" then
      return si
    end
  end
  return nil
end

local function lookup_node_by_name (om, name)
  if name == nil then
    return nil
  end

  for si in om:iterate { type = "SiLinkable" } do
    if si.properties ["node.name"] == name then
      return si
    end
  end
  return nil
end

local function is_physical_alsa_sink (si)
  local props = si.properties
  if props ["media.class"] ~= "Audio/Sink" then
    return false
  end
  if not starts_with (props ["node.name"] or "", "alsa_output.") then
    return false
  end
  if props ["node.virtual"] == "true" then
    return false
  end
  return true
end

-- Pick the physical ALSA sink that actually drives the speakers.
-- Preference order:
--   1. The configured/default audio sink, when it points at an
--      `alsa_output.*` node (i.e. the user has not selected a virtual
--      sink as default).
--   2. The highest-priority `alsa_output.*` SiLinkable in the graph.
-- Returning the physical sink (instead of `jamesdsp_sink`) means the
-- AEC reference is captured from the post-effect monitor that matches
-- exactly what the speakers play, regardless of how many virtual
-- chains (JamesDSP, output-biglinux, future processors) sit upstream.
local function lookup_physical_sink (source, om)
  local metadata = metadata_object (source, "default")
  local configured = json_name (metadata, "default.configured.audio.sink")
  local candidate = lookup_sink_by_name (om, configured)
  if candidate and is_physical_alsa_sink (candidate) then
    return candidate
  end

  local default_name = json_name (metadata, "default.audio.sink")
  candidate = lookup_sink_by_name (om, default_name)
  if candidate and is_physical_alsa_sink (candidate) then
    return candidate
  end

  local best = nil
  local best_priority = -1
  for si in om:iterate { type = "SiLinkable" } do
    if is_physical_alsa_sink (si) then
      local props = si.properties
      local priority = tonumber (props ["priority.session"])
          or tonumber (props ["priority.driver"])
          or tonumber (props ["node.id"])
          or 0
      if priority > best_priority then
        best = si
        best_priority = priority
      end
    end
  end
  return best
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

-- Pin the AEC reference to the physical ALSA sink that actually
-- drives the speakers. With `monitor.mode = true`, the AEC module's
-- internal capture stream (`echo-cancel-sink`) follows the default
-- sink, but a smart filter (`output-biglinux`) wraps that default and
-- the link ends up at `output-biglinux:monitor` — the *pre-effect*
-- mix. Any post-default-sink processor (JamesDSP, future convolvers,
-- a hardware EQ on the ALSA card) shifts the speaker signal further
-- away from that monitor, so the canceller subtracts the wrong
-- reference and echo bleeds through.
--
-- Targeting the ALSA sink monitor instead means the reference is
-- whatever the kernel hands to the DAC — exactly what comes back into
-- the mic — regardless of how many virtual processors sit upstream.
SimpleEventHook {
  name = "biglinux/echo-cancel-sink-target",
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

    if si_props ["node.name"] ~= EC_SINK_NODE_NAME then
      return
    end

    -- Note: do NOT bail when `target` is already set. The AEC module
    -- runs with `monitor.mode = true`, so the default sink is baked
    -- into `target.object` and resolved before this hook fires. We
    -- still want to replace that pre-resolved target with the
    -- physical ALSA sink.
    local candidate = lookup_physical_sink (source, om)
    if not candidate then
      log:info (si, "no physical ALSA sink found, leaving AEC reference as-is")
      return
    end

    if not lutils.canLink (si_props, candidate) then
      log:warning (si, "physical sink is not linkable as AEC reference")
      return
    end

    local passthrough_compatible, can_passthrough =
        lutils.checkPassthroughCompatibility (si, candidate)
    if not passthrough_compatible then
      log:warning (si, "physical sink passthrough incompatible")
      return
    end

    -- Both flags pinned: `has_defined_target` makes the linker treat
    -- our choice as the resolved target; `has_node_defined_target`
    -- makes `linking/find-filter-target` treat it as user-pinned and
    -- skip the smart-filter rewrap that would otherwise re-insert
    -- `output-biglinux:monitor` between our target and the AEC stream.
    si_flags.has_defined_target = true
    si_flags.has_node_defined_target = true
    si_flags.can_passthrough = can_passthrough
    event:set_data ("target", candidate)
    log:info (si,
      "routing AEC reference to "
      .. tostring (candidate.properties ["node.name"]))
  end
}:register ()

-- Keep `output-biglinux`'s smart-filter target aligned with the sink
-- that apps are really going to. The conf bakes
-- `filter.smart.target = { node.name = "alsa_output.*" }`, which works
-- when the default sink is the physical ALSA output. When JamesDSP is
-- running, its daemon either moves app streams onto `jamesdsp_sink`
-- directly or the user picks `jamesdsp_sink` as default — in both
-- cases the smart filter no longer wraps the right sink and apps skip
-- the BigLinux EQ/HPF/gate entirely. We override the smart target via
-- the public `filters` metadata so WirePlumber re-wraps apps that aim
-- at `jamesdsp_sink`.
local function maybe_retarget_output_smart_filter (source, om)
  local filter_si = lookup_node_by_name (om, OUTPUT_FILTER_NODE_NAME)
  if filter_si == nil then
    return
  end
  local filter_node = filter_si:get_associated_proxy ("node")
  if filter_node == nil then
    return
  end
  local filter_id = filter_node ["bound-id"]
  if filter_id == nil then
    return
  end

  local metadata = metadata_object (source, "filters")
  if metadata == nil then
    return
  end

  if lookup_sink_by_name (om, JAMESDSP_SINK_NAME) then
    metadata:set (filter_id, "filter.smart.target", "Spa:String:JSON",
      "{ \"node.name\": \"" .. JAMESDSP_SINK_NAME .. "\" }")
  else
    -- Clear the override so the conf-supplied alsa target wins.
    metadata:set (filter_id, "filter.smart.target", nil, nil)
  end
end

SimpleEventHook {
  name = "biglinux/output-smart-filter-retarget",
  before = "linking/rescan-trigger",
  interests = {
    EventInterest {
      Constraint { "event.type", "=", "session-item-added" },
      Constraint { "event.session-item.interface", "=", "linkable" },
    },
  },
  execute = function (event)
    maybe_retarget_output_smart_filter (
      event:get_source (),
      event:get_source ():call ("get-object-manager", "session-item"))
  end
}:register ()

-- When `jamesdsp_sink` materialises after WirePlumber has already
-- linked `echo-cancel-sink` to a stale target, the AEC reference
-- stays wrong until the next unrelated rescan. Force a rescan as soon
-- as we see the new linkable so the sink-target hook above re-runs
-- and `maybe_retarget_output_smart_filter` re-evaluates routing.
SimpleEventHook {
  name = "biglinux/jamesdsp-sink-rescan",
  before = "linking/rescan-trigger",
  interests = {
    EventInterest {
      Constraint { "event.type", "=", "session-item-added" },
      Constraint { "event.session-item.interface", "=", "linkable" },
    },
  },
  execute = function (event)
    local si = event:get_subject ()
    local node = si:get_associated_proxy ("node")
    if node == nil then
      return
    end
    local name = node.properties ["node.name"]
    if name ~= JAMESDSP_SINK_NAME then
      return
    end
    -- Schedule a rescan so existing streams (echo-cancel-sink,
    -- output-biglinux smart-filter) re-evaluate their routing now
    -- that the JamesDSP sink is part of the graph.
    local source = event:get_source ()
    source:call ("schedule-rescan", "linking")
    log:info ("scheduled rescan after jamesdsp_sink appeared")
  end
}:register ()
