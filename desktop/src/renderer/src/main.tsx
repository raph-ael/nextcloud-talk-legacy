import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import type { TalkApi } from '@shared/core'
import { App } from './App'
import { boot } from './store'
import { startMediaEngine } from './call/engine'
import '@fontsource-variable/inter'
import './styles.css'

declare global {
  interface Window {
    talk: TalkApi
  }
}

void boot()
startMediaEngine()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
