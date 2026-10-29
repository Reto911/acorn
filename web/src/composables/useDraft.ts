import {
  computed,
  onScopeDispose,
  reactive,
  readonly,
  ref,
  shallowRef,
  watch,
} from 'vue'
import type { LanguageId, Problem } from '../api/types'
import { exceptionContext, log } from '../utils/log'

/** Local draft format, independent of the frozen submission request. */
interface DraftRecord {
  source: string
  version_no: number
  updated_at_ms: number
}

interface Draft {
  source: string
  version: number
  savedAt: number | null
  dirty: boolean
}

type DraftStorage = Pick<Storage, 'getItem' | 'setItem' | 'removeItem'>

function browserStorage(): DraftStorage | null {
  try {
    return window.localStorage
  } catch {
    return null
  }
}

/** Validate stored fields without printing source text or raw storage contents. */
function decodeDraft(text: string): DraftRecord {
  const value: unknown = JSON.parse(text)
  if (
    typeof value !== 'object' ||
    value === null ||
    !('source' in value) ||
    typeof value.source !== 'string' ||
    !('version_no' in value) ||
    typeof value.version_no !== 'number' ||
    !Number.isSafeInteger(value.version_no) ||
    value.version_no < 1 ||
    !('updated_at_ms' in value) ||
    typeof value.updated_at_ms !== 'number' ||
    !Number.isFinite(value.updated_at_ms)
  )
    throw new Error('Invalid saved draft')
  return {
    source: value.source,
    version_no: value.version_no,
    updated_at_ms: value.updated_at_ms,
  }
}

/** One answer page owns its drafts, debounce timer and last selected language. */
export function useDraft(
  problem: () => Problem,
  owner: () => number | null,
  storage: DraftStorage | null = browserStorage(),
) {
  const problemId = problem().problem_id
  let ownerId = owner()
  const languageId = ref<LanguageId>(problem().languages[0]!.language_id)
  const storageWarning = ref<string | null>(null)
  const versionNotice = ref<string | null>(null)
  const drafts = new Map<LanguageId, Draft>()
  const anonymousEdits = new Set<LanguageId>()
  let anonymousLanguageSelected = false
  let timer: ReturnType<typeof setTimeout> | undefined
  let notifiedVersion: number | undefined

  function failure(operation: string, message: string, cause?: unknown) {
    storageWarning.value = message
    log('warn', { operation, ...exceptionContext(cause) })
  }

  function draftKey(language: LanguageId) {
    return `acorn:draft:v2:${ownerId}:${problemId}:${language}`
  }

  function languageKey() {
    return `acorn:draft-language:v2:${ownerId}:${problemId}`
  }

  function restoreLanguage() {
    languageId.value = problem().languages[0]!.language_id
    if (!storage || ownerId === null) return
    try {
      const selected = storage.getItem(languageKey())
      if (problem().languages.some((item) => item.language_id === selected))
        languageId.value = selected as LanguageId
    } catch (cause) {
      failure(
        'restore-draft-language',
        '无法恢复上次选择的语言，请重新选择；本地草稿可能无法读取。',
        cause,
      )
    }
  }

  restoreLanguage()
  if (!storage && ownerId !== null) {
    failure(
      'open-draft-storage',
      '本地存储不可用，草稿仅保留在当前页面内存中，刷新或关闭后可能丢失。',
    )
  }

  function load(language: LanguageId): Draft {
    const cached = drafts.get(language)
    if (cached) return cached
    const draft = reactive<Draft>({
      source: '',
      version: problem().version_no,
      savedAt: null,
      dirty: false,
    })
    if (storage && ownerId !== null) {
      try {
        const text = storage.getItem(draftKey(language))
        if (text !== null) {
          const saved = decodeDraft(text)
          draft.source = saved.source
          draft.version = saved.version_no
          draft.savedAt = saved.updated_at_ms
        }
      } catch (cause) {
        failure(
          'restore-draft',
          '无法读取本语言的本地草稿，编辑区从空内容开始；原存储尚未改动。',
          cause,
        )
      }
    }
    drafts.set(language, draft)
    return draft
  }

  const current = shallowRef(load(languageId.value))

  function cancelTimer() {
    clearTimeout(timer)
    timer = undefined
  }

  function saveDraft(language: LanguageId, draft: Draft) {
    if (ownerId === null) {
      storageWarning.value = '尚未登录，草稿仅保留在当前页面内存中。'
      return false
    }
    if (!storage) {
      draft.dirty = true
      failure('save-draft', '本地存储不可用，草稿未保存；当前仍可编辑和提交。')
      return false
    }
    const savedAt = Date.now()
    try {
      storage.setItem(
        draftKey(language),
        JSON.stringify({
          source: draft.source,
          version_no: draft.version,
          updated_at_ms: savedAt,
        } satisfies DraftRecord),
      )
      storage.setItem(languageKey(), languageId.value)
      draft.savedAt = savedAt
      draft.dirty = false
      storageWarning.value = null
      return true
    } catch (cause) {
      draft.dirty = true
      failure(
        'save-draft',
        '无法保存本地草稿，当前内容尚未保存；请复制代码留存，编辑和提交仍可继续。',
        cause,
      )
      return false
    }
  }

  function save() {
    cancelTimer()
    return saveDraft(languageId.value, current.value)
  }

  function flush() {
    cancelTimer()
    let warning: string | null = null
    for (const [language, draft] of drafts) {
      if (draft.dirty && !saveDraft(language, draft))
        warning = storageWarning.value
    }
    if (warning) storageWarning.value = warning
  }

  function syncVersion() {
    const version = problem().version_no
    if (current.value.version === version) return
    if (notifiedVersion !== version) {
      versionNotice.value = `题面已从 v${current.value.version} 更新至 v${version}，代码草稿已保留，请复核新题面。`
      notifiedVersion = version
    }
    current.value.version = version
    current.value.dirty = true
    save()
  }

  function selectLanguage(language: LanguageId) {
    if (language === languageId.value) return
    if (ownerId === null) anonymousLanguageSelected = true
    flush()
    languageId.value = language
    current.value = load(language)
    syncVersion()
    if (storage && ownerId !== null) {
      try {
        storage.setItem(languageKey(), language)
      } catch (cause) {
        failure(
          'save-draft-language',
          '无法保存语言选择，下次打开时可能需要重新选择。',
          cause,
        )
      }
    }
  }

  const source = computed({
    get: () => current.value.source,
    set: (value: string) => {
      if (value === current.value.source) return
      if (ownerId === null) anonymousEdits.add(languageId.value)
      current.value.source = value
      current.value.dirty = true
      cancelTimer()
      timer = setTimeout(save, 1000)
    },
  })
  const saved = computed(
    () => !current.value.dirty && current.value.savedAt !== null,
  )

  /** Call after confirmation; only this problem and language are cleared. */
  function clearConfirmed() {
    cancelTimer()
    current.value.source = ''
    current.value.savedAt = null
    current.value.dirty = true
    if (ownerId === null) {
      anonymousEdits.add(languageId.value)
      current.value.dirty = false
      storageWarning.value = null
      return true
    }
    try {
      if (!storage) throw new Error('Draft storage is unavailable')
      storage.removeItem(draftKey(languageId.value))
      current.value.dirty = false
      storageWarning.value = null
      return true
    } catch (cause) {
      failure(
        'clear-draft',
        '编辑区已清空，但无法清除本地草稿；刷新后可能恢复旧内容。',
        cause,
      )
      return false
    }
  }

  watch(
    owner,
    (nextOwner) => {
      const previousOwner = ownerId
      // Save with the captured owner before changing any storage key.
      flush()
      ownerId = nextOwner
      storageWarning.value = null
      if (previousOwner === null && nextOwner !== null) {
        // Anonymous buffers, including other edited languages, survive login.
        const selectedLanguage = languageId.value
        const edited = [...drafts].filter(([language]) =>
          anonymousEdits.has(language),
        )
        drafts.clear()
        restoreLanguage()
        if (anonymousLanguageSelected || anonymousEdits.has(selectedLanguage))
          languageId.value = selectedLanguage
        for (const [language, draft] of edited) {
          draft.dirty = true
          drafts.set(language, draft)
        }
        current.value = load(languageId.value)
        anonymousEdits.clear()
        anonymousLanguageSelected = false
        syncVersion()
        flush()
        return
      }
      drafts.clear()
      anonymousEdits.clear()
      anonymousLanguageSelected = false
      versionNotice.value = null
      notifiedVersion = undefined
      restoreLanguage()
      current.value = load(languageId.value)
      syncVersion()
    },
    { flush: 'sync' },
  )

  watch(
    problem,
    () => {
      if (
        !problem().languages.some(
          (item) => item.language_id === languageId.value,
        )
      )
        selectLanguage(problem().languages[0]!.language_id)
      syncVersion()
    },
    { flush: 'sync' },
  )
  syncVersion()
  onScopeDispose(() => {
    flush()
    cancelTimer()
  })

  return {
    source,
    languageId: readonly(languageId),
    saved,
    storageWarning: readonly(storageWarning),
    versionNotice: readonly(versionNotice),
    selectLanguage,
    save,
    flush,
    clearConfirmed,
    dismissVersionNotice: () => {
      versionNotice.value = null
    },
  }
}
