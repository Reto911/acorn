import {
  createSSRApp,
  defineComponent,
  h,
  ref,
  type Ref,
  type SetupContext,
} from 'vue'
import { renderToString } from 'vue/server-renderer'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import AuthDialog from './AuthDialog.vue'

const auth = {
  dialogOpen: ref(true),
  busy: ref(false),
  formError: ref<unknown>(null),
  login: vi.fn(),
  register: vi.fn(),
  closeDialog: vi.fn(),
  clearFormError: vi.fn(),
}

vi.mock('../composables/useAuth', () => ({ useAuth: () => auth }))
vi.mock('element-plus/es/components/alert/style/css', () => ({}))
vi.mock('element-plus/es/components/button/style/css', () => ({}))
vi.mock('element-plus/es/components/checkbox/style/css', () => ({}))
vi.mock('element-plus/es/components/dialog/style/css', () => ({}))
vi.mock('element-plus/es/components/form/style/css', () => ({}))
vi.mock('element-plus/es/components/form-item/style/css', () => ({}))
vi.mock('element-plus/es/components/input/style/css', () => ({}))

interface DialogActions {
  form: { username: string; password: string; display_name: string }
  formRef: Ref<
    { validate: () => Promise<boolean>; clearValidate: () => void } | undefined
  >
  mode: Ref<'login' | 'register'>
  notice: Ref<string>
  submit: () => Promise<void>
  switchMode: () => Promise<void>
}

/** Node's existing SSR setup exposes component actions without adding a DOM runtime. */
async function actions() {
  let result: DialogActions | undefined
  const setup = (
    AuthDialog as unknown as {
      setup: (props: object, context: SetupContext) => DialogActions
    }
  ).setup
  await renderToString(
    createSSRApp(
      defineComponent({
        setup(props, context) {
          result = setup(props, context)
          return () => h('div')
        },
      }),
    ),
  )
  if (!result) throw new Error('Missing authentication actions')
  const validate = vi.fn().mockResolvedValue(true)
  result.formRef.value = { validate, clearValidate: vi.fn() }
  return { dialog: result, validate }
}

beforeEach(() => {
  vi.clearAllMocks()
  auth.dialogOpen.value = true
  auth.busy.value = false
  auth.formError.value = null
  auth.login.mockResolvedValue(true)
  auth.register.mockResolvedValue(true)
})

describe('authentication dialog actions', () => {
  it('waits for Element Plus validation and sends no request for invalid input', async () => {
    const { dialog, validate } = await actions()
    validate
      .mockResolvedValueOnce(false)
      .mockRejectedValueOnce(new Error('Invalid field'))
    await dialog.submit()
    await dialog.submit()
    expect(validate).toHaveBeenCalledTimes(2)
    expect(auth.login).not.toHaveBeenCalled()
    expect(auth.register).not.toHaveBeenCalled()
  })

  it('returns successful registration to login without automatically logging in', async () => {
    const { dialog } = await actions()
    await dialog.switchMode()
    dialog.form.username = ' \tTest_1\r\n'
    dialog.form.password = 'correct-password'
    await dialog.submit()
    expect(auth.register).toHaveBeenCalledExactlyOnceWith({
      username: 'test_1',
      password: 'correct-password',
    })
    expect(auth.login).not.toHaveBeenCalled()
    expect(auth.closeDialog).not.toHaveBeenCalled()
    expect(dialog.mode.value).toBe('login')
    expect(dialog.notice.value).toBe('注册成功，请使用用户名和密码手动登录。')
    expect(dialog.form.password).toBe('')
    dialog.form.password = 'correct-password'
    await dialog.submit()
    expect(auth.login).toHaveBeenCalledExactlyOnceWith({
      username: 'test_1',
      password: 'correct-password',
    })
    expect(auth.closeDialog).toHaveBeenCalledOnce()
  })
})
