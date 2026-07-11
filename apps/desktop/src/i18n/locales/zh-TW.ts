export const zhTW = {
  common: { confirm: '確認', cancel: '取消', delete: '刪除', edit: '編輯', add: '新增', back: '返回' },
  errorBoundary: {
    title: '介面渲染錯誤',
    description: '應用程式發生執行階段錯誤。請將下方資訊回報給開發者，或重新啟動應用程式。',
    retry: '重試',
  },
  sidebar: {
    brand: 'OpenWork', sessions: '對話', emptySessions: '暫無對話', newSession: '建立對話',
    untitledSession: '新對話', settings: '設定', backToApp: '返回 OpenWork', settingsNavigation: '設定導覽',
    collapse: '收合側欄', expand: '展開側欄', renameSession: '重新命名對話', deleteSession: '刪除對話',
    deleteSessionPrompt: '刪除此對話？', sessionName: '對話名稱',
  },
  header: { sessionActions: '對話操作' },
  chat: {
    placeholder: '想問些什麼……', selectModel: '選擇模型', noModel: '暫無模型', send: '傳送', stop: '停止生成',
    approvalMode: '審批模式：由我審批', approveForMe: '由我審批', executionPermission: '執行權限',
    approvalDescription: '工具呼叫前暫停，等待你確認允許或拒絕。', startConversation: '開始對話', createSession: '建立對話',
    noSessionHelp: '點選側欄的「建立對話」按鈕，開始使用 OpenWork。',
    readyHelp: '開始新的程式開發對話。OpenWork 已準備好協助你建置、除錯及梳理專案。',
    providerRequired: '請先在設定中配置並啟用模型供應商，再開始新的程式開發對話。', waiting: '正在等待回應',
  },
  settings: {
    title: '設定',
    models: {
      title: '模型配置', description: '新增模型供應商並啟用一項配置，以開始對話。', providers: '模型供應商',
      addProvider: '新增供應商', empty: '尚未配置模型供應商。點選「新增供應商」開始配置。', active: '使用中',
      setActive: '設為目前供應商', test: '測試連線', edit: '編輯供應商', delete: '刪除供應商', noModels: '暫無模型',
      testFailed: '測試失敗：{{message}}', deleteConfirm: '確定刪除供應商「{{name}}」嗎？',
      form: {
        addTitle: '新增模型供應商', editTitle: '編輯模型供應商', close: '關閉', preset: '預設', name: '名稱',
        baseUrl: '請求位址', apiKey: 'API 金鑰', showApiKey: '顯示 API 金鑰', hideApiKey: '隱藏 API 金鑰',
        liteModels: 'Lite 模型', plusModels: 'Plus 模型', proModels: 'Pro 模型', extraBody: '附加請求參數',
        test: '測試', testing: '正在測試', connectivityOk: '連線成功', save: '儲存',
        nameRequired: '請輸入名稱', baseUrlRequired: '請輸入請求位址', apiKeyRequired: '請輸入 API 金鑰',
        duplicateModel: '模型 {{model}} 被分配至多個強度等級', extraBodyObject: '附加請求參數必須是 JSON 物件',
        extraBodyInvalid: '附加請求參數不是有效的 JSON', modelRequiredForTest: '請至少新增一個模型後再測試', failed: '失敗：{{message}}',
      },
    },
    appearance: {
      title: '外觀', description: '選擇 OpenWork 在此裝置上的顯示方式。', themeLabel: '主題', light: '亮色',
      lightDescription: '一律使用亮色外觀。', dark: '深色', darkDescription: '一律使用深色外觀。', system: '跟隨系統',
      systemDescription: '跟隨作業系統的外觀設定。', language: '語言', languageDescription: '選擇 OpenWork 的介面語言。',
      simplifiedChinese: '简体中文', traditionalChinese: '繁體中文', english: 'English',
    },
  },
  tool: {
    thinking: '思考中', thought: '思考過程', running: '執行中', done: '已完成', result: '結果', error: '錯誤', input: '輸入',
    content: '內容', noInput: '無輸入', waitingApproval: '等待審批', processing: '處理中……', allow: '允許', reject: '拒絕',
    allowBash: '允許執行 Bash 指令', allowWrite: '允許寫入 {{name}}', allowWriteFile: '允許寫入檔案',
    allowRead: '允許讀取 {{name}}', allowReadFile: '允許讀取檔案', allowList: '允許列出 {{name}}',
    allowListDirectory: '允許列出目錄', allowTool: '允許工具 {{name}}',
  },
} as const
