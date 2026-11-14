import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import type { TalkApi } from '@shared/core'
import { App } from './App'
import { boot } from './store'
import { loadAppearance } from './appearance'
import { startMediaEngine } from './call/engine'
import { startCallUi } from './call/store'
import '@fontsource-variable/inter'
import './styles.css'

declare global {
  interface Window {
    talk: TalkApi
  }
}

void boot()
startMediaEngine()
startCallUi()

// The accent is known before anything is drawn in it (the theme already is: main process).
void loadAppearance().finally(() =>
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  ),
)
