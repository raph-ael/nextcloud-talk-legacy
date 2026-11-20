import { createPluginRuntimeStore } from "openclaw/plugin-sdk/runtime-store";

export const CHANNEL_ID = "nctalk";

const store = createPluginRuntimeStore({
  pluginId: CHANNEL_ID,
  errorMessage: "nctalk: Laufzeit ist nicht initialisiert",
});

export const setNctalkRuntime = store.setRuntime;
export const getNctalkRuntime = store.getRuntime;
export const getOptionalNctalkRuntime = store.tryGetRuntime;
