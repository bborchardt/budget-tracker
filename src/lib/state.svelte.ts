// App-wide state shared by the views.

export type View = 'dashboard' | 'plan' | 'transactions' | 'import' | 'savings' | 'settings'

export const app = $state({
  view: 'dashboard' as View,
  year: new Date().getFullYear(),
  /** Bumped after any change so views reload their data */
  version: 0,
  /** Transaction filter requested by another view (e.g. dashboard -> category) */
  transactionFilter: null as null | { category_id?: number; auto_only?: boolean; pending_only?: boolean },
})

export function changed() {
  app.version++
}

export function go(view: View) {
  app.view = view
}
