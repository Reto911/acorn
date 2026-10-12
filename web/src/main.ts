import { createApp } from 'vue'
import App from './App.vue'
import { router } from './router'
import { installErrorHandlers } from './utils/browserErrors'
import './styles/main.css'

const app = createApp(App)
installErrorHandlers(app)
app.use(router).mount('#app')
