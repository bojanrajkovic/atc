type Theme = 'warm' | 'radar' | 'violet' | 'pink'
type Mode = 'dark' | 'light'
type Density = 'compact' | 'comfortable'

class UIStore {
  theme = $state<Theme>('radar')
  mode = $state<Mode>('dark')
  density = $state<Density>('comfortable')
  selectedRunId = $state<bigint | null>(null)

  constructor() {
    // Restore persisted values from localStorage
    if (typeof window !== 'undefined') {
      const savedTheme = localStorage.getItem('atc-theme') as Theme | null
      const savedMode = localStorage.getItem('atc-mode') as Mode | null
      const savedDensity = localStorage.getItem('atc-density') as Density | null
      if (savedTheme) this.theme = savedTheme
      if (savedMode) this.mode = savedMode
      if (savedDensity) this.density = savedDensity
    }

    // Sync to DOM and localStorage via $effect
    // Use $effect.root() because this is a module-level singleton outside component context
    $effect.root(() => {
      $effect(() => {
        document.documentElement.setAttribute('data-theme', this.theme)
        localStorage.setItem('atc-theme', this.theme)
      })

      $effect(() => {
        if (this.mode === 'light') {
          document.documentElement.setAttribute('data-mode', 'light')
        } else {
          document.documentElement.removeAttribute('data-mode')
        }
        localStorage.setItem('atc-mode', this.mode)
      })

      $effect(() => {
        if (this.density === 'compact') {
          document.documentElement.setAttribute('data-density', 'compact')
        } else {
          document.documentElement.removeAttribute('data-density')
        }
        localStorage.setItem('atc-density', this.density)
      })
    })
  }
}

export const uiStore = new UIStore()
