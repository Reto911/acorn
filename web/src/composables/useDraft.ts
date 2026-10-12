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
  storage: DraftStorage | null = browserStorage(),
) {
  const problemId = problem().problem_id
  const languageKey = `acorn:draft-language:v1:${problemId}`
  const languageId = ref<LanguageId>(problem().languages[0]!.language_id)
  const storageWarning = ref<string | null>(null)
  const versionNotice = ref<string | null>(null)
  const drafts = new Map<LanguageId, Draft>()
  let timer: ReturnType<typeof setTimeout> | undefined
  let notifiedVersion: number | undefined

  function failure(operation: string, message: string, cause?: unknown) {
    storageWarning.value = message
    log('warn', { operation, ...exceptionContext(cause) })
  }

  function draftKey(language: LanguageId) {
    return `acorn:draft:v1:${problemId}:${language}`
  }

  if (storage) {
    try {
      const selected = storage.getItem(languageKey)
      if (problem().languages.some((item) => item.language_id === selected))
        languageId.value = selected as LanguageId
    } catch (cause) {
      failure(
        'restore-draft-language',
        '无法恢复上次选择的语言，请重新选择；本地草稿可能无法读取。',
        cause,
      )
    }
  } else {
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
    if (storage) {
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

  function save() {
    cancelTimer()
    if (!storage) {
      current.value.dirty = true
      failure('save-draft', '本地存储不可用，草稿未保存；当前仍可编辑和提交。')
      return false
    }
    const savedAt = Date.now()
    try {
      storage.setItem(
        draftKey(languageId.value),
        JSON.stringify({
          source: current.value.source,
          version_no: current.value.version,
          updated_at_ms: savedAt,
        } satisfies DraftRecord),
      )
      storage.setItem(languageKey, languageId.value)
      current.value.savedAt = savedAt
      current.value.dirty = false
      storageWarning.value = null
      return true
    } catch (cause) {
      current.value.dirty = true
      failure(
        'save-draft',
        '无法保存本地草稿，当前内容尚未保存；请复制代码留存，编辑和提交仍可继续。',
        cause,
      )
      return false
    }
  }

  function flush() {
    if (current.value.dirty) save()
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
    flush()
    languageId.value = language
    current.value = load(language)
    syncVersion()
    if (storage) {
      try {
        storage.setItem(languageKey, language)
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
