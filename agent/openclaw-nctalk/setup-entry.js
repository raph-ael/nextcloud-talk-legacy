import { defineSetupPluginEntry } from "openclaw/plugin-sdk/channel-core";
import { nctalkPlugin } from "./src/channel.js";

export default defineSetupPluginEntry(nctalkPlugin);
