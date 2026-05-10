import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { PaletteStore } from './palette.svelte'

describe('PaletteStore', () => {
  let store: PaletteStore
  const extraStores: PaletteStore[] = []

  beforeEach(() => {
    sessionStorage.clear()
    store = new PaletteStore()
  })

  afterEach(() => {
    // Tear down the persistence $effect so it doesn't keep ticking and writing
    // to sessionStorage between tests under `isolate: false`. Tests that create
    // additional PaletteStore instances push them onto extraStores so they get
    // cleaned up symmetrically.
    store.destroy()
    while (extraStores.length > 0) {
      const s = extraStores.pop()
      s?.destroy()
    }
  })

  // Test 1: Defaults
  it('initializes with correct defaults', () => {
    expect(store.paletteOpen).toBe(false)
    expect(store.paletteQuery).toBe('')
    expect(store.recentRunIds).toEqual([])
    expect(store.subMenu).toBeNull()
  })

  // Test 2: open() preserves query
  it('open() preserves paletteQuery', () => {
    store.paletteQuery = 'foo'
    store.open()
    expect(store.paletteQuery).toBe('foo')
    expect(store.paletteOpen).toBe(true)
  })

  // Test 3: close() clears submenu
  it('close() clears subMenu and closes palette', () => {
    store.subMenu = 'theme'
    store.paletteOpen = true
    store.close()
    expect(store.subMenu).toBeNull()
    expect(store.paletteOpen).toBe(false)
  })

  // Test 4: toggle() from closed → open preserves query
  it('toggle() from closed to open preserves query', () => {
    store.paletteQuery = 'test-query'
    expect(store.paletteOpen).toBe(false)
    store.toggle()
    expect(store.paletteOpen).toBe(true)
    expect(store.paletteQuery).toBe('test-query')
  })

  // Test 5: toggle() from open → closed clears submenu
  it('toggle() from open to closed clears submenu', () => {
    store.paletteOpen = true
    store.subMenu = 'theme'
    store.toggle()
    expect(store.paletteOpen).toBe(false)
    expect(store.subMenu).toBeNull()
  })

  // Test 6: recordRunVisit(id) adds to head when new
  it('recordRunVisit() adds new id to head', () => {
    const id = 123n
    store.recordRunVisit(id)
    expect(store.recentRunIds[0]).toBe(id)
  })

  // Test 7: recordRunVisit(id) moves existing to head
  it('recordRunVisit() moves existing id to head', () => {
    store.recordRunVisit(1n)
    store.recordRunVisit(2n)
    store.recordRunVisit(1n)
    expect(store.recentRunIds).toEqual([1n, 2n])
  })

  // Test 8: LRU cap at 10
  it('caps recentRunIds at 10 items', () => {
    for (let i = 1n; i <= 12n; i++) {
      store.recordRunVisit(i)
    }
    expect(store.recentRunIds).toHaveLength(10)
    expect(store.recentRunIds[0]).toBe(12n)
  })

  // Test 9: sessionStorage persistence
  it('persists recentRunIds to sessionStorage and restores on new instance', async () => {
    // Manually set up sessionStorage with test data
    sessionStorage.clear()
    const testStore = new PaletteStore()
    extraStores.push(testStore)
    testStore.recordRunVisit(100n)
    testStore.recordRunVisit(200n)
    // Wait for effect to fire
    await new Promise((r) => setTimeout(r, 0))

    // Tear the writer down before constructing the reader so its effect can't
    // race the reader's initial sessionStorage read under `isolate: false`.
    testStore.destroy()
    extraStores.pop()

    // Create new instance reading from the persisted data
    const store2 = new PaletteStore()
    extraStores.push(store2)
    expect(store2.recentRunIds).toEqual([200n, 100n])
  })

  // Test 10: sessionStorage handles bigint round-trip
  it('correctly round-trips large bigints through sessionStorage', async () => {
    // Manually set up sessionStorage
    sessionStorage.clear()
    const testStore = new PaletteStore()
    extraStores.push(testStore)
    const largeId = 9007199254740993n // > Number.MAX_SAFE_INTEGER
    testStore.recordRunVisit(largeId)
    // Wait for effect
    await new Promise((r) => setTimeout(r, 0))

    testStore.destroy()
    extraStores.pop()

    const store2 = new PaletteStore()
    extraStores.push(store2)
    expect(store2.recentRunIds[0]).toBe(largeId)
    expect(store2.recentRunIds[0] === largeId).toBe(true)
  })

  // Test 11: enterSubmenu('theme') sets subMenu
  it('enterSubmenu() sets subMenu', () => {
    store.enterSubmenu('theme')
    expect(store.subMenu).toBe('theme')
  })

  // Test 12: exitSubmenu() clears subMenu
  it('exitSubmenu() clears subMenu without affecting paletteOpen', () => {
    store.paletteOpen = true
    store.subMenu = 'theme'
    store.exitSubmenu()
    expect(store.subMenu).toBeNull()
    expect(store.paletteOpen).toBe(true)
  })

  // Test 13: sessionStorage SecurityError on getItem doesn't crash construction
  it('survives sessionStorage.getItem throwing on construction', () => {
    const original = Storage.prototype.getItem
    Storage.prototype.getItem = () => {
      throw new DOMException('storage blocked', 'SecurityError')
    }
    try {
      const testStore = new PaletteStore()
      extraStores.push(testStore)
      expect(testStore.recentRunIds).toEqual([])
    } finally {
      Storage.prototype.getItem = original
    }
  })

  // Test 14: sessionStorage QuotaExceededError on setItem doesn't propagate
  it('survives sessionStorage.setItem throwing on persistence', async () => {
    const original = Storage.prototype.setItem
    Storage.prototype.setItem = () => {
      throw new DOMException('quota exceeded', 'QuotaExceededError')
    }
    try {
      const testStore = new PaletteStore()
      extraStores.push(testStore)
      // Should not throw — the effect catches the storage error silently
      expect(() => testStore.recordRunVisit(1n)).not.toThrow()
      await new Promise((r) => setTimeout(r, 0))
      // Internal state is updated even though persistence dropped
      expect(testStore.recentRunIds).toEqual([1n])
    } finally {
      Storage.prototype.setItem = original
    }
  })
})
