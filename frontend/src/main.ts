import './app.css'
import { mount } from 'svelte'
import App from './App.svelte'

const appElement = document.getElementById('app')
if (!appElement) {
  throw new Error('Could not find root element with id="app"')
}

const app = mount(App, {
  target: appElement,
})

export default app
