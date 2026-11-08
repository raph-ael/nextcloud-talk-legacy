import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import type { TalkApi } from '@shared/core'
import { App } from './App'
import { boot } from './store'
import './styles.css'

declare global {
  interface Window {
    talk: TalkApi
  }
}

void boot()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
