import { defineChannelPluginEntry } from "openclaw/plugin-sdk/channel-core";
import { nctalkPlugin } from "./src/channel.js";
import { setNctalkRuntime } from "./src/runtime.js";

export default defineChannelPluginEntry({
  id: "nctalk",
  name: "Nextcloud Talk (Benutzerkonto)",
  description: "Nextcloud Talk als Kanal, bei dem der Agent ein echtes Nextcloud-Benutzerkonto ist.",
  plugin: nctalkPlugin,
  setRuntime: setNctalkRuntime,
});
