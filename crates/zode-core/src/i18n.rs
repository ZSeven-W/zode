//! Runtime i18n for the Zode TUI. Covers the 15 languages of the OpenPencil
//! ecosystem (en, zh, zh-tw, ja, ko, es, fr, de, pt, ru, hi, id, th, tr, vi).
//!
//! Design: the English string IS the key. [`t`] wraps a string literal and
//! returns its translation for the currently-selected language, falling back
//! to the English literal when there's no translation. So wiring a UI string
//! for i18n is just `t("Settings")` instead of `"Settings"`. The current
//! language lives in a process-global so a `/language` (Settings) switch takes
//! effect on the next render without threading state through every widget.
//!
//! Translation data lives in the generated [`i18n_data`] module
//! (`SOURCE_STRINGS` ⇄ `LOCALES`, aligned by index).

use std::sync::RwLock;

use crate::i18n_data::{LOCALES, SOURCE_STRINGS};

/// Hand-maintained translations for UI strings added after the bulk locale
/// table was generated (so new strings don't require regenerating the big
/// index-aligned arrays). Each row is `[en, zh, zh-tw, ja, ko, es, fr, de, pt,
/// ru, hi, id, th, tr, vi]`, aligned to [`Lang::ALL`]. Checked before
/// `SOURCE_STRINGS`. Key names (enter/esc) are intentionally left out — they
/// pass through unchanged.
#[rustfmt::skip]
const EXTRA: &[[&str; 15]] = &[
    ["Discover and register installed external agent CLIs", "发现并注册已安装的外部代理 CLI", "探索並註冊已安裝的外部代理 CLI", "インストール済みの外部エージェント CLI を検出して登録", "설치된 외부 에이전트 CLI 검색 및 등록", "Detectar y registrar las CLI de agentes externos instaladas", "Découvrir et enregistrer les CLI d’agents externes installées", "Installierte externe Agent-CLIs erkennen und registrieren", "Descobrir e registrar CLIs de agentes externos instaladas", "Найти и зарегистрировать установленные CLI внешних агентов", "इंस्टॉल किए गए बाहरी एजेंट CLI खोजें और रजिस्टर करें", "Temukan dan daftarkan CLI agen eksternal yang terpasang", "ค้นหาและลงทะเบียน CLI เอเจนต์ภายนอกที่ติดตั้งไว้", "Kurulu harici ajan CLI'larını keşfet ve kaydet", "Phát hiện và đăng ký các CLI tác nhân bên ngoài đã cài đặt"],
    // /connect dialog.
    ["Configured", "已配置", "已設定", "設定済み", "구성됨", "Configurados", "Configurés", "Konfiguriert", "Configurados", "Настроенные", "कॉन्फ़िगर किए गए", "Terkonfigurasi", "ที่กำหนดค่าแล้ว", "Yapılandırılmış", "Đã cấu hình"],
    ["No providers", "无提供商", "無供應商", "プロバイダーなし", "공급자 없음", "Sin proveedores", "Aucun fournisseur", "Keine Anbieter", "Sem provedores", "Нет провайдеров", "कोई प्रदाता नहीं", "Tidak ada penyedia", "ไม่มีผู้ให้บริการ", "Sağlayıcı yok", "Không có nhà cung cấp"],
    // NB: "Provider" intentionally omitted — it already lives in SOURCE_STRINGS
    // (used by settings); duplicating it here would silently override that.
    ["API key", "API 密钥", "API 金鑰", "API キー", "API 키", "Clave API", "Clé API", "API-Schlüssel", "Chave API", "API-ключ", "API कुंजी", "Kunci API", "คีย์ API", "API anahtarı", "Khóa API"],
    ["type", "类型", "類型", "タイプ", "유형", "tipo", "type", "Typ", "tipo", "тип", "प्रकार", "tipe", "ประเภท", "tür", "loại"],
    ["model", "模型", "模型", "モデル", "모델", "modelo", "modèle", "Modell", "modelo", "модель", "मॉडल", "model", "โมเดล", "model", "mô hình"],
    ["base URL", "基础 URL", "基礎 URL", "ベース URL", "기본 URL", "URL base", "URL de base", "Basis-URL", "URL base", "Базовый URL", "बेस URL", "URL dasar", "URL พื้นฐาน", "Temel URL", "URL cơ sở"],
    ["context", "上下文", "上下文", "コンテキスト", "컨텍스트", "contexto", "contexte", "Kontext", "contexto", "контекст", "संदर्भ", "konteks", "บริบท", "bağlam", "ngữ cảnh"],
    ["max output", "最大输出", "最大輸出", "最大出力", "최대 출력", "salida máx.", "sortie max", "max. Ausgabe", "saída máx.", "макс. вывод", "अधिकतम आउटपुट", "keluaran maks", "เอาต์พุตสูงสุด", "maks çıktı", "đầu ra tối đa"],
    ["input $/M", "输入 $/M", "輸入 $/M", "入力 $/M", "입력 $/M", "entrada $/M", "entrée $/M", "Eingabe $/M", "entrada $/M", "ввод $/M", "इनपुट $/M", "input $/M", "อินพุต $/M", "girdi $/M", "đầu vào $/M"],
    ["output $/M", "输出 $/M", "輸出 $/M", "出力 $/M", "출력 $/M", "salida $/M", "sortie $/M", "Ausgabe $/M", "saída $/M", "вывод $/M", "आउटपुट $/M", "output $/M", "เอาต์พุต $/M", "çıktı $/M", "đầu ra $/M"],
    ["submit", "提交", "提交", "送信", "제출", "enviar", "envoyer", "senden", "enviar", "отправить", "सबमिट करें", "kirim", "ส่ง", "gönder", "gửi"],
    ["finishing background work… (Ctrl+C to force quit)", "正在完成后台工作…(Ctrl+C 强制退出)", "正在完成背景工作…(Ctrl+C 強制退出)", "バックグラウンド処理を完了しています…(Ctrl+C で強制終了)", "백그라운드 작업 마무리 중…(Ctrl+C로 강제 종료)", "finalizando el trabajo en segundo plano… (Ctrl+C para forzar la salida)", "finalisation des tâches en arrière-plan… (Ctrl+C pour forcer la sortie)", "Hintergrundarbeit wird abgeschlossen… (Strg+C zum Erzwingen des Beendens)", "concluindo o trabalho em segundo plano… (Ctrl+C para forçar a saída)", "завершение фоновой работы… (Ctrl+C для принудительного выхода)", "पृष्ठभूमि कार्य पूरा हो रहा है… (बाहर निकलने के लिए Ctrl+C)", "menyelesaikan pekerjaan latar belakang… (Ctrl+C untuk keluar paksa)", "กำลังทำงานเบื้องหลังให้เสร็จ… (Ctrl+C เพื่อบังคับออก)", "arka plan işi tamamlanıyor… (zorla çıkmak için Ctrl+C)", "đang hoàn tất công việc nền… (Ctrl+C để buộc thoát)"],
    // Status-bar mode labels new since the table was generated. (ready/running/
    // idle/thinking/streaming/error already live in the generated table — their
    // zh placeholders are fixed there, in i18n_data.rs, not here.)
    ["compacting", "压缩中", "壓縮中", "圧縮中", "압축 중", "compactando", "compactage", "komprimiert", "compactando", "сжатие", "संकुचन", "memadatkan", "กำลังบีบอัด", "sıkıştırılıyor", "đang nén"],
    ["switching", "切换中", "切換中", "切り替え中", "전환 중", "cambiando", "changement", "wechselt", "alternando", "переключение", "स्विच कर रहा है", "beralih", "กำลังสลับ", "değiştiriliyor", "đang chuyển"],
    // Sidebar section headers + value labels (were hardcoded English).
    ["session", "会话", "會話", "セッション", "세션", "sesión", "session", "Sitzung", "sessão", "сессия", "सत्र", "sesi", "เซสชัน", "oturum", "phiên"],
    ["workspace", "工作区", "工作區", "ワークスペース", "작업 공간", "espacio de trabajo", "espace de travail", "Arbeitsbereich", "espaço de trabalho", "рабочая область", "कार्यक्षेत्र", "ruang kerja", "พื้นที่ทำงาน", "çalışma alanı", "không gian làm việc"],
    ["goal", "目标", "目標", "目標", "목표", "objetivo", "objectif", "Ziel", "objetivo", "цель", "लक्ष्य", "tujuan", "เป้าหมาย", "hedef", "mục tiêu"],
    ["cost", "花费", "花費", "コスト", "비용", "costo", "coût", "Kosten", "custo", "стоимость", "लागत", "biaya", "ค่าใช้จ่าย", "maliyet", "chi phí"],
    ["theme", "主题", "主題", "テーマ", "테마", "tema", "thème", "Thema", "tema", "тема", "थीम", "tema", "ธีม", "tema", "chủ đề"],
    ["tokens", "令牌", "權杖", "トークン", "토큰", "tokens", "jetons", "Tokens", "tokens", "токены", "टोकन", "token", "โทเค็น", "token", "token"],
    ["looping", "循环中", "循環中", "ループ中", "반복 중", "en bucle", "en boucle", "Schleife", "em loop", "цикл", "लूप में", "berulang", "กำลังวนซ้ำ", "döngüde", "đang lặp"],
    ["standard", "标准", "標準", "標準", "표준", "estándar", "standard", "Standard", "padrão", "стандарт", "मानक", "standar", "มาตรฐาน", "standart", "tiêu chuẩn"],
    ["sandbox", "沙盒", "沙盒", "サンドボックス", "샌드박스", "sandbox", "bac à sable", "Sandbox", "sandbox", "песочница", "सैंडबॉक्स", "sandbox", "แซนด์บ็อกซ์", "kum havuzu", "hộp cát"],
    // ── Bulk i18n coverage pass (auto-audited untranslated UI strings). ──
    ["ctx", "上下文", "上下文", "コンテキスト", "컨텍스트", "contexto", "contexte", "Kontext", "contexto", "контекст", "संदर्भ", "konteks", "บริบท", "bağlam", "ngữ cảnh"],
    ["YOLO", "YOLO", "YOLO", "YOLO", "YOLO", "YOLO", "YOLO", "YOLO", "YOLO", "YOLO", "YOLO", "YOLO", "YOLO", "YOLO", "YOLO"],
    ["SANDBOX:RO", "沙盒:只读", "沙盒:唯讀", "サンドボックス:読取専用", "샌드박스:읽기전용", "SANDBOX:SL", "BAC À SABLE:LS", "SANDBOX:NUR-LESEN", "SANDBOX:SL", "ПЕСОЧНИЦА:ЧТ", "SANDBOX:RO", "SANDBOX:RO", "แซนด์บ็อกซ์:อ่านอย่างเดียว", "SANDBOX:SO", "SANDBOX:CHỈĐỌC"],
    ["SANDBOX", "沙盒", "沙盒", "サンドボックス", "샌드박스", "SANDBOX", "BAC À SABLE", "SANDBOX", "SANDBOX", "ПЕСОЧНИЦА", "सैंडबॉक्स", "SANDBOX", "แซนด์บ็อกซ์", "SANDBOX", "SANDBOX"],
    ["NET", "网络", "網路", "ネット", "네트워크", "RED", "RÉSEAU", "NETZ", "REDE", "СЕТЬ", "नेट", "NET", "เน็ต", "AĞ", "MẠNG"],
    ["UNSANDBOXED", "无沙盒", "無沙盒", "サンドボックスなし", "샌드박스 없음", "SIN SANDBOX", "SANS BAC À SABLE", "OHNE SANDBOX", "SEM SANDBOX", "БЕЗ ПЕСОЧНИЦЫ", "बिना सैंडबॉक्स", "TANPA SANDBOX", "ไม่มีแซนด์บ็อกซ์", "SANDBOX YOK", "KHÔNG SANDBOX"],
    ["PLAN", "计划", "計畫", "プラン", "플랜", "PLAN", "PLAN", "PLAN", "PLANO", "ПЛАН", "योजना", "RENCANA", "แผน", "PLAN", "KẾ HOẠCH"],
    ["SELECT", "选择", "選取", "選択", "선택", "SELECCIÓN", "SÉLECTION", "AUSWAHL", "SELECIONAR", "ВЫБОР", "चयन", "PILIH", "เลือก", "SEÇ", "CHỌN"],
    ["Todo", "待办", "待辦", "ToDo", "할 일", "Tareas", "Tâches", "Aufgaben", "Tarefas", "Задачи", "कार्य", "Tugas", "สิ่งที่ต้องทำ", "Yapılacaklar", "Việc cần làm"],
    ["no active todos", "无活动待办", "無進行中待辦", "アクティブなタスクなし", "활성 할 일 없음", "sin tareas activas", "aucune tâche active", "keine aktiven Aufgaben", "sem tarefas ativas", "нет активных задач", "कोई सक्रिय कार्य नहीं", "tidak ada tugas aktif", "ไม่มีงานที่ทำอยู่", "aktif görev yok", "không có việc đang làm"],
    ["more", "更多", "更多", "他", "더", "más", "de plus", "mehr", "mais", "ещё", "और", "lagi", "เพิ่มเติม", "daha", "nữa"],
    ["subagents", "子代理", "子代理", "サブエージェント", "서브에이전트", "subagentes", "sous-agents", "Unteragenten", "subagentes", "субагенты", "सब-एजेंट", "subagen", "ซับเอเจนต์", "alt aracılar", "tác nhân con"],
    ["workbench", "工作台", "工作台", "ワークベンチ", "작업대", "banco de trabajo", "établi", "Werkbank", "bancada", "рабочее место", "वर्कबेंच", "meja kerja", "โต๊ะทำงาน", "çalışma tezgahı", "bàn làm việc"],
    ["terminal coding agent", "终端编程代理", "終端編程代理", "ターミナルコーディングエージェント", "터미널 코딩 에이전트", "agente de codificación de terminal", "agent de codage en terminal", "Terminal-Coding-Agent", "agente de codificação de terminal", "терминальный агент для кодирования", "टर्मिनल कोडिंग एजेंट", "agen pengodean terminal", "เอเจนต์เขียนโค้ดบนเทอร์มินัล", "terminal kodlama ajanı", "tác nhân lập trình terminal"],
    ["ready for code, shells, and agents", "为代码、Shell 和代理就绪", "為程式碼、Shell 和代理就緒", "コード、シェル、エージェントの準備完了", "코드, 셸, 에이전트 준비 완료", "listo para código, shells y agentes", "prêt pour le code, les shells et les agents", "bereit für Code, Shells und Agenten", "pronto para código, shells e agentes", "готов к коду, оболочкам и агентам", "कोड, शेल और एजेंट के लिए तैयार", "siap untuk kode, shell, dan agen", "พร้อมสำหรับโค้ด เชลล์ และเอเจนต์", "kod, kabuklar ve ajanlar için hazır", "sẵn sàng cho mã, shell và tác nhân"],
    ["small mascot, serious tools", "小吉祥物，硬核工具", "小吉祥物，硬核工具", "小さなマスコット、本格ツール", "작은 마스코트, 진지한 도구", "mascota pequeña, herramientas serias", "petite mascotte, outils sérieux", "kleines Maskottchen, ernsthafte Werkzeuge", "mascote pequeno, ferramentas sérias", "маленький маскот, серьёзные инструменты", "छोटा शुभंकर, गंभीर उपकरण", "maskot kecil, alat serius", "มาสคอตตัวเล็ก เครื่องมือจริงจัง", "küçük maskot, ciddi araçlar", "linh vật nhỏ, công cụ nghiêm túc"],
    ["command deck online", "指挥台已上线", "指揮台已上線", "コマンドデッキ オンライン", "커맨드 덱 온라인", "puente de mando en línea", "poste de commande en ligne", "Kommandozentrale online", "painel de comando online", "командный пункт в сети", "कमांड डेक ऑनलाइन", "dek komando daring", "แผงบังคับการออนไลน์", "komuta güvertesi çevrimiçi", "đài chỉ huy trực tuyến"],
    ["neon rails online", "霓虹轨道已上线", "霓虹軌道已上線", "ネオンレール オンライン", "네온 레일 온라인", "rieles neón en línea", "rails néon en ligne", "Neon-Schienen online", "trilhos neon online", "неоновые рельсы в сети", "नियॉन रेल्स ऑनलाइन", "rel neon daring", "รางนีออนออนไลน์", "neon raylar çevrimiçi", "đường ray neon trực tuyến"],
    ["slash commands are hot", "斜杠命令已激活", "斜線命令已啟用", "スラッシュコマンド有効", "슬래시 명령 활성화됨", "los comandos de barra están activos", "les commandes slash sont actives", "Slash-Befehle sind aktiv", "comandos de barra estão ativos", "слэш-команды активны", "स्लैश कमांड सक्रिय हैं", "perintah garis miring aktif", "คำสั่งสแลชพร้อมใช้งาน", "eğik çizgi komutları aktif", "lệnh gạch chéo đang bật"],
    ["(thinking & tool details are hidden — /thinking or /tool-details to show them)", "（思考与工具详情已隐藏 — 使用 /thinking 或 /tool-details 显示）", "（思考與工具詳情已隱藏 — 使用 /thinking 或 /tool-details 顯示）", "（思考とツールの詳細は非表示 — /thinking または /tool-details で表示）", "(사고 및 도구 세부 정보가 숨겨짐 — /thinking 또는 /tool-details 로 표시)", "(los detalles de razonamiento y herramientas están ocultos — /thinking o /tool-details para mostrarlos)", "(les détails de réflexion et d'outils sont masqués — /thinking ou /tool-details pour les afficher)", "(Denk- und Tool-Details sind ausgeblendet — /thinking oder /tool-details zum Anzeigen)", "(detalhes de raciocínio e ferramentas estão ocultos — /thinking ou /tool-details para mostrá-los)", "(детали размышлений и инструментов скрыты — /thinking или /tool-details чтобы показать)", "(सोच और टूल विवरण छिपे हैं — दिखाने के लिए /thinking या /tool-details)", "(detail pemikiran & alat disembunyikan — /thinking atau /tool-details untuk menampilkannya)", "(รายละเอียดการคิดและเครื่องมือถูกซ่อน — /thinking หรือ /tool-details เพื่อแสดง)", "(düşünme ve araç ayrıntıları gizli — göstermek için /thinking veya /tool-details)", "(chi tiết suy nghĩ & công cụ đang ẩn — /thinking hoặc /tool-details để hiện)"],
    ["diff preview truncated", "差异预览已截断", "差異預覽已截斷", "差分プレビューを省略しました", "diff 미리보기가 잘렸습니다", "vista previa de diff truncada", "aperçu du diff tronqué", "Diff-Vorschau gekürzt", "prévia do diff truncada", "предпросмотр diff усечён", "diff पूर्वावलोकन काट दिया गया", "pratinjau diff dipotong", "ตัวอย่าง diff ถูกตัดทอน", "diff önizlemesi kısaltıldı", "bản xem trước diff bị cắt bớt"],
    ["allow once", "允许一次", "允許一次", "1回許可", "한 번 허용", "permitir una vez", "autoriser une fois", "einmal erlauben", "permitir uma vez", "разрешить один раз", "एक बार अनुमति दें", "izinkan sekali", "อนุญาตครั้งเดียว", "bir kez izin ver", "cho phép một lần"],
    ["allow always", "始终允许", "一律允許", "常に許可", "항상 허용", "permitir siempre", "toujours autoriser", "immer erlauben", "permitir sempre", "разрешать всегда", "हमेशा अनुमति दें", "selalu izinkan", "อนุญาตเสมอ", "her zaman izin ver", "luôn cho phép"],
    ["deny", "拒绝", "拒絕", "拒否", "거부", "denegar", "refuser", "ablehnen", "negar", "отклонить", "अस्वीकार करें", "tolak", "ปฏิเสธ", "reddet", "từ chối"],
    ["…or just keep typing to queue a message — this prompt won't block you.", "…或者继续输入以排队发送消息 — 此提示不会阻塞你。", "…或者繼續輸入以排隊傳送訊息 — 此提示不會阻擋你。", "…または入力を続けてメッセージをキューに追加できます — このプロンプトは操作を妨げません。", "…또는 계속 입력하여 메시지를 대기열에 추가하세요 — 이 프롬프트는 작업을 막지 않습니다.", "…o sigue escribiendo para poner un mensaje en cola: este aviso no te bloqueará.", "…ou continuez à taper pour mettre un message en file d'attente — cette invite ne vous bloque pas.", "…oder tippe einfach weiter, um eine Nachricht einzureihen — diese Eingabeaufforderung blockiert dich nicht.", "…ou continue digitando para enfileirar uma mensagem — este aviso não vai bloquear você.", "…или просто продолжайте печатать, чтобы поставить сообщение в очередь — это приглашение не блокирует вас.", "…या संदेश को कतार में डालने के लिए टाइप करते रहें — यह प्रॉम्प्ट आपको नहीं रोकेगा।", "…atau terus mengetik untuk mengantrekan pesan — prompt ini tidak akan memblokir Anda.", "…หรือพิมพ์ต่อเพื่อจัดคิวข้อความ — พร้อมต์นี้จะไม่ขัดขวางคุณ", "…veya bir mesajı sıraya almak için yazmaya devam edin — bu istem sizi engellemez.", "…hoặc cứ tiếp tục gõ để xếp hàng một tin nhắn — lời nhắc này sẽ không chặn bạn."],
    ["The agent is asking", "智能体正在询问", "代理正在詢問", "エージェントが質問しています", "에이전트가 질문하고 있습니다", "El agente está preguntando", "L'agent pose une question", "Der Agent fragt", "O agente está perguntando", "Агент спрашивает", "एजेंट पूछ रहा है", "Agen sedang bertanya", "เอเจนต์กำลังถาม", "Aracı soruyor", "Tác nhân đang hỏi"],
    ["Other (type a custom answer)", "其他（输入自定义答案）", "其他（輸入自訂答案）", "その他（カスタム回答を入力）", "기타 (사용자 지정 답변 입력)", "Otro (escribe una respuesta personalizada)", "Autre (saisir une réponse personnalisée)", "Andere (eigene Antwort eingeben)", "Outro (digite uma resposta personalizada)", "Другое (введите свой ответ)", "अन्य (कस्टम उत्तर टाइप करें)", "Lainnya (ketik jawaban khusus)", "อื่น ๆ (พิมพ์คำตอบที่กำหนดเอง)", "Diğer (özel bir yanıt yazın)", "Khác (nhập câu trả lời tùy chỉnh)"],
    ["Submit", "提交", "提交", "送信", "제출", "Enviar", "Envoyer", "Absenden", "Enviar", "Отправить", "सबमिट करें", "Kirim", "ส่ง", "Gönder", "Gửi"],
    ["Submit (answer all questions first)", "提交（请先回答所有问题）", "提交（請先回答所有問題）", "送信（先にすべての質問に回答してください）", "제출 (먼저 모든 질문에 답하세요)", "Enviar (responde primero todas las preguntas)", "Envoyer (répondez d'abord à toutes les questions)", "Absenden (zuerst alle Fragen beantworten)", "Enviar (responda todas as perguntas primeiro)", "Отправить (сначала ответьте на все вопросы)", "सबमिट करें (पहले सभी प्रश्नों के उत्तर दें)", "Kirim (jawab semua pertanyaan dulu)", "ส่ง (ตอบทุกคำถามก่อน)", "Gönder (önce tüm soruları yanıtlayın)", "Gửi (trả lời tất cả câu hỏi trước)"],
    ["custom answer", "自定义答案", "自訂答案", "カスタム回答", "사용자 지정 답변", "respuesta personalizada", "réponse personnalisée", "eigene Antwort", "resposta personalizada", "свой ответ", "कस्टम उत्तर", "jawaban khusus", "คำตอบที่กำหนดเอง", "özel yanıt", "câu trả lời tùy chỉnh"],
    ["confirm", "确认", "確認", "確定", "확인", "confirmar", "confirmer", "bestätigen", "confirmar", "подтвердить", "पुष्टि करें", "konfirmasi", "ยืนยัน", "onayla", "xác nhận"],
    // Session picker delete confirmation.
    ["confirm delete", "确认删除", "確認刪除", "削除を確認", "삭제 확인", "confirmar eliminación", "confirmer la suppression", "Löschen bestätigen", "confirmar exclusão", "подтвердить удаление", "हटाने की पुष्टि करें", "konfirmasi hapus", "ยืนยันการลบ", "silmeyi onayla", "xác nhận xóa"],
    ["stop editing", "停止编辑", "停止編輯", "編集を終了", "편집 중지", "dejar de editar", "arrêter la modification", "Bearbeitung beenden", "parar de editar", "закончить редактирование", "संपादन रोकें", "berhenti mengedit", "หยุดแก้ไข", "düzenlemeyi durdur", "dừng chỉnh sửa"],
    ["move", "移动", "移動", "移動", "이동", "mover", "déplacer", "bewegen", "mover", "переместить", "ले जाएँ", "pindah", "เลื่อน", "taşı", "di chuyển"],
    ["skip", "跳过", "跳過", "スキップ", "건너뛰기", "omitir", "passer", "überspringen", "pular", "пропустить", "छोड़ें", "lewati", "ข้าม", "atla", "bỏ qua"],
    ["Sessions", "会话", "會話", "セッション", "세션", "Sesiones", "Sessions", "Sitzungen", "Sessões", "Сессии", "सत्र", "Sesi", "เซสชัน", "Oturumlar", "Phiên"],
    ["resume", "恢复", "恢復", "再開", "재개", "reanudar", "reprendre", "fortsetzen", "retomar", "возобновить", "फिर से शुरू करें", "lanjutkan", "กลับมาทำต่อ", "devam et", "tiếp tục"],
    ["killed", "已终止", "已終止", "終了済み", "종료됨", "terminado", "arrêté", "beendet", "encerrado", "завершён", "समाप्त", "dihentikan", "ถูกยุติ", "sonlandırıldı", "đã kết thúc"],
    ["Background shells", "后台 shell", "背景 shell", "バックグラウンドシェル", "백그라운드 셸", "Shells en segundo plano", "Shells en arrière-plan", "Hintergrund-Shells", "Shells em segundo plano", "Фоновые оболочки", "बैकग्राउंड शेल", "Shell latar belakang", "เชลล์เบื้องหลัง", "Arka plan kabukları", "Shell nền"],
    ["kill", "终止", "終止", "終了", "종료", "terminar", "arrêter", "beenden", "encerrar", "завершить", "समाप्त करें", "hentikan", "ยุติ", "sonlandır", "kết thúc"],
    ["(no running turns)", "（没有正在运行的回合）", "（沒有正在執行的回合）", "（実行中のターンはありません）", "(실행 중인 턴 없음)", "(sin turnos en ejecución)", "(aucun tour en cours)", "(keine laufenden Durchläufe)", "(nenhum turno em execução)", "(нет активных ходов)", "(कोई चालू टर्न नहीं)", "(tidak ada giliran berjalan)", "(ไม่มีเทิร์นที่กำลังทำงาน)", "(çalışan tur yok)", "(không có lượt đang chạy)"],
    ["Running turns", "运行中的回合", "執行中的回合", "実行中のターン", "실행 중인 턴", "Turnos en ejecución", "Tours en cours", "Laufende Durchläufe", "Turnos em execução", "Активные ходы", "चल रहे टर्न", "Giliran yang berjalan", "เทิร์นที่กำลังทำงาน", "Çalışan turlar", "Lượt đang chạy"],
    ["Manage plugins", "管理插件", "管理外掛", "プラグインを管理", "플러그인 관리", "Gestionar complementos", "Gérer les plugins", "Plugins verwalten", "Gerenciar plugins", "Управление плагинами", "प्लगइन प्रबंधित करें", "Kelola plugin", "จัดการปลั๊กอิน", "Eklentileri yönet", "Quản lý plugin"],
    ["No plugins match", "没有匹配的插件", "沒有符合的外掛", "一致するプラグインがありません", "일치하는 플러그인 없음", "No hay complementos coincidentes", "Aucun plugin correspondant", "Keine passenden Plugins", "Nenhum plugin corresponde", "Нет подходящих плагинов", "कोई मिलान प्लगइन नहीं", "Tidak ada plugin yang cocok", "ไม่มีปลั๊กอินที่ตรงกัน", "Eşleşen eklenti yok", "Không có plugin phù hợp"],
    ["Installed plugins", "已安装插件", "已安裝外掛", "インストール済みプラグイン", "설치된 플러그인", "Complementos instalados", "Plugins installés", "Installierte Plugins", "Plugins instalados", "Установленные плагины", "इंस्टॉल किए गए प्लगइन", "Plugin terpasang", "ปลั๊กอินที่ติดตั้ง", "Yüklü eklentiler", "Plugin đã cài đặt"],
    ["Tool groups", "工具组", "工具群組", "ツールグループ", "도구 그룹", "Grupos de herramientas", "Groupes d'outils", "Werkzeuggruppen", "Grupos de ferramentas", "Группы инструментов", "टूल समूह", "Grup alat", "กลุ่มเครื่องมือ", "Araç grupları", "Nhóm công cụ"],
    ["Skills", "技能", "技能", "スキル", "스킬", "Habilidades", "Compétences", "Fähigkeiten", "Habilidades", "Навыки", "कौशल", "Keterampilan", "สกิล", "Beceriler", "Kỹ năng"],
    ["LSP servers", "LSP 服务器", "LSP 伺服器", "LSP サーバー", "LSP 서버", "Servidores LSP", "Serveurs LSP", "LSP-Server", "Servidores LSP", "Серверы LSP", "LSP सर्वर", "Server LSP", "เซิร์ฟเวอร์ LSP", "LSP sunucuları", "Máy chủ LSP"],
    ["Sub-agents", "子智能体", "子代理", "サブエージェント", "서브 에이전트", "Subagentes", "Sous-agents", "Unteragenten", "Subagentes", "Субагенты", "उप-एजेंट", "Sub-agen", "เอเจนต์ย่อย", "Alt aracılar", "Tác nhân con"],
    ["scroll", "滚动", "捲動", "スクロール", "스크롤", "desplazar", "défiler", "scrollen", "rolar", "прокрутка", "स्क्रॉल करें", "gulir", "เลื่อน", "kaydır", "cuộn"],
    ["no sub-agents yet", "暂无子智能体", "尚無子代理", "サブエージェントはまだありません", "아직 서브 에이전트 없음", "aún no hay subagentes", "aucun sous-agent pour l'instant", "noch keine Unteragenten", "ainda não há subagentes", "пока нет субагентов", "अभी तक कोई उप-एजेंट नहीं", "belum ada sub-agen", "ยังไม่มีเอเจนต์ย่อย", "henüz alt aracı yok", "chưa có tác nhân con"],
    ["report connection state", "报告连接状态", "回報連線狀態", "接続状態を報告", "연결 상태 보고", "informar estado de conexión", "signaler l'état de connexion", "Verbindungsstatus melden", "relatar estado da conexão", "сообщить состояние подключения", "कनेक्शन स्थिति रिपोर्ट करें", "laporkan status koneksi", "รายงานสถานะการเชื่อมต่อ", "bağlantı durumunu bildir", "báo cáo trạng thái kết nối"],
    ["run batch_design DSL", "运行 batch_design DSL", "執行 batch_design DSL", "batch_design DSL を実行", "batch_design DSL 실행", "ejecutar batch_design DSL", "exécuter batch_design DSL", "batch_design DSL ausführen", "executar batch_design DSL", "выполнить batch_design DSL", "batch_design DSL चलाएँ", "jalankan batch_design DSL", "รัน batch_design DSL", "batch_design DSL çalıştır", "chạy batch_design DSL"],
    ["generate a page from a prompt", "根据提示生成页面", "根據提示生成頁面", "プロンプトからページを生成", "프롬프트에서 페이지 생성", "generar una página a partir de un prompt", "générer une page à partir d'un prompt", "eine Seite aus einem Prompt generieren", "gerar uma página a partir de um prompt", "создать страницу из запроса", "प्रॉम्प्ट से पेज बनाएँ", "buat halaman dari prompt", "สร้างหน้าจากพรอมต์", "bir istemden sayfa oluştur", "tạo trang từ lời nhắc"],
    ["insert a node", "插入节点", "插入節點", "ノードを挿入", "노드 삽입", "insertar un nodo", "insérer un nœud", "einen Knoten einfügen", "inserir um nó", "вставить узел", "नोड डालें", "sisipkan node", "แทรกโหนด", "bir düğüm ekle", "chèn một nút"],
    ["update node properties", "更新节点属性", "更新節點屬性", "ノードのプロパティを更新", "노드 속성 업데이트", "actualizar propiedades del nodo", "mettre à jour les propriétés du nœud", "Knoteneigenschaften aktualisieren", "atualizar propriedades do nó", "обновить свойства узла", "नोड गुण अपडेट करें", "perbarui properti node", "อัปเดตคุณสมบัติโหนด", "düğüm özelliklerini güncelle", "cập nhật thuộc tính nút"],
    ["delete nodes", "删除节点", "刪除節點", "ノードを削除", "노드 삭제", "eliminar nodos", "supprimer des nœuds", "Knoten löschen", "excluir nós", "удалить узлы", "नोड हटाएँ", "hapus node", "ลบโหนด", "düğümleri sil", "xóa các nút"],
    ["move nodes", "移动节点", "移動節點", "ノードを移動", "노드 이동", "mover nodos", "déplacer des nœuds", "Knoten verschieben", "mover nós", "переместить узлы", "नोड ले जाएँ", "pindahkan node", "ย้ายโหนด", "düğümleri taşı", "di chuyển các nút"],
    ["copy nodes", "复制节点", "複製節點", "ノードをコピー", "노드 복사", "copiar nodos", "copier des nœuds", "Knoten kopieren", "copiar nós", "копировать узлы", "नोड कॉपी करें", "salin node", "คัดลอกโหนด", "düğümleri kopyala", "sao chép các nút"],
    ["page operations", "页面操作", "頁面操作", "ページ操作", "페이지 작업", "operaciones de página", "opérations de page", "Seitenoperationen", "operações de página", "операции со страницей", "पेज संचालन", "operasi halaman", "การดำเนินการกับหน้า", "sayfa işlemleri", "thao tác trang"],
    ["variable operations", "变量操作", "變數操作", "変数操作", "변수 작업", "operaciones de variables", "opérations sur les variables", "Variablenoperationen", "operações de variáveis", "операции с переменными", "चर संचालन", "operasi variabel", "การดำเนินการกับตัวแปร", "değişken işlemleri", "thao tác biến"],
    ["selection operations", "选区操作", "選取操作", "選択操作", "선택 작업", "operaciones de selección", "opérations de sélection", "Auswahloperationen", "operações de seleção", "операции с выделением", "चयन संचालन", "operasi seleksi", "การดำเนินการกับการเลือก", "seçim işlemleri", "thao tác lựa chọn"],
    ["call any MCP tool by name", "按名称调用任意 MCP 工具", "依名稱呼叫任意 MCP 工具", "名前で任意の MCP ツールを呼び出す", "이름으로 아무 MCP 도구 호출", "llamar cualquier herramienta MCP por nombre", "appeler n'importe quel outil MCP par son nom", "beliebiges MCP-Tool per Namen aufrufen", "chamar qualquer ferramenta MCP pelo nome", "вызвать любой инструмент MCP по имени", "नाम से कोई भी MCP टूल कॉल करें", "panggil alat MCP apa pun berdasarkan nama", "เรียกใช้เครื่องมือ MCP ใดก็ได้ตามชื่อ", "herhangi bir MCP aracını adıyla çağır", "gọi bất kỳ công cụ MCP nào theo tên"],
    ["Currency", "货币", "貨幣", "通貨", "통화", "Moneda", "Devise", "Währung", "Moeda", "Валюта", "मुद्रा", "Mata uang", "สกุลเงิน", "Para birimi", "Tiền tệ"],
    ["Display currency", "显示货币", "顯示貨幣", "表示通貨", "표시 통화", "Moneda de visualización", "Devise d'affichage", "Anzeigewährung", "Moeda de exibição", "Валюта отображения", "प्रदर्शन मुद्रा", "Mata uang tampilan", "สกุลเงินที่แสดง", "Görüntüleme para birimi", "Tiền tệ hiển thị"],
    // Sidebar MCP + modified-files sections ("mcp" itself passes through).
    ["modified files", "修改文件", "修改檔案", "変更ファイル", "수정된 파일", "archivos modificados", "fichiers modifiés", "geänderte Dateien", "arquivos modificados", "изменённые файлы", "संशोधित फ़ाइलें", "berkas diubah", "ไฟล์ที่แก้ไข", "değişen dosyalar", "tệp đã sửa"],
    ["connected", "已连接", "已連線", "接続済み", "연결됨", "conectado", "connecté", "verbunden", "conectado", "подключено", "कनेक्टेड", "terhubung", "เชื่อมต่อแล้ว", "bağlı", "đã kết nối"],
    ["disconnected", "未连接", "未連線", "未接続", "연결 안 됨", "desconectado", "déconnecté", "getrennt", "desconectado", "отключено", "डिस्कनेक्टेड", "terputus", "ไม่ได้เชื่อมต่อ", "bağlı değil", "chưa kết nối"],
    // Question-dialog tab strip.
    ["switch", "切换", "切換", "切替", "전환", "cambiar", "changer", "wechseln", "alternar", "переключить", "स्विच", "beralih", "สลับ", "geçiş", "chuyển"],
    // ── Ghost-cursor overlay + desktop Esc stop. ──
    ["zode is controlling your computer", "zode 正在操控你的电脑", "zode 正在操控你的電腦", "zode がこのコンピュータを操作しています", "zode가 컴퓨터를 조작하고 있습니다", "zode está controlando tu computadora", "zode contrôle votre ordinateur", "zode steuert Ihren Computer", "zode está controlando seu computador", "zode управляет вашим компьютером", "zode आपका कंप्यूटर नियंत्रित कर रहा है", "zode sedang mengendalikan komputer Anda", "zode กำลังควบคุมคอมพิวเตอร์ของคุณ", "zode bilgisayarınızı kontrol ediyor", "zode đang điều khiển máy tính của bạn"],
    ["press Esc to stop", "按 Esc 停止", "按 Esc 停止", "Esc で停止", "Esc 키로 중지", "presiona Esc para detener", "appuyez sur Échap pour arrêter", "Esc zum Stoppen", "pressione Esc para parar", "нажмите Esc для остановки", "रोकने के लिए Esc दबाएं", "tekan Esc untuk berhenti", "กด Esc เพื่อหยุด", "durdurmak için Esc'ye basın", "nhấn Esc để dừng"],
    ["typing…", "输入中…", "輸入中…", "入力中…", "입력 중…", "escribiendo…", "saisie…", "Eingabe…", "digitando…", "ввод…", "टाइपिंग…", "mengetik…", "กำลังพิมพ์…", "yazılıyor…", "đang gõ…"],
    ["desktop automation stopped (Esc)", "桌面自动化已停止（Esc）", "桌面自動化已停止（Esc）", "デスクトップ自動化を停止しました（Esc）", "데스크톱 자동화 중지됨 (Esc)", "automatización de escritorio detenida (Esc)", "automatisation du bureau arrêtée (Échap)", "Desktop-Automatisierung gestoppt (Esc)", "automação de desktop parada (Esc)", "автоматизация рабочего стола остановлена (Esc)", "डेस्कटॉप स्वचालन रोका गया (Esc)", "otomasi desktop dihentikan (Esc)", "การทำงานอัตโนมัติของเดสก์ท็อปหยุดแล้ว (Esc)", "masaüstü otomasyonu durduruldu (Esc)", "tự động hóa máy tính đã dừng (Esc)"],
    // ── Turn completion footer (elapsed time + tool count). ──
    ["✓ done · {duration} · {n} tools", "✓ 完成 · {duration} · {n} 个工具", "✓ 完成 · {duration} · {n} 個工具", "✓ 完了 · {duration} · ツール {n} 件", "✓ 완료 · {duration} · 도구 {n}개", "✓ hecho · {duration} · {n} herramientas", "✓ terminé · {duration} · {n} outils", "✓ fertig · {duration} · {n} Tools", "✓ concluído · {duration} · {n} ferramentas", "✓ готово · {duration} · {n} инструментов", "✓ पूर्ण · {duration} · {n} टूल", "✓ selesai · {duration} · {n} alat", "✓ เสร็จ · {duration} · เครื่องมือ {n} รายการ", "✓ bitti · {duration} · {n} araç", "✓ xong · {duration} · {n} công cụ"],
    // ── /loop + /schedule TUI wiring. ──
    ["loop started: every {interval} (id {id})", "循环已启动：每 {interval}（id {id}）", "循環已啟動：每 {interval}（id {id}）", "ループ開始：{interval} ごと（id {id}）", "루프 시작됨: {interval}마다 (id {id})", "bucle iniciado: cada {interval} (id {id})", "boucle démarrée : toutes les {interval} (id {id})", "Loop gestartet: alle {interval} (id {id})", "loop iniciado: a cada {interval} (id {id})", "цикл запущен: каждые {interval} (id {id})", "लूप शुरू: हर {interval} (id {id})", "loop dimulai: setiap {interval} (id {id})", "เริ่มลูปแล้ว: ทุก {interval} (id {id})", "döngü başladı: her {interval} (id {id})", "vòng lặp đã bắt đầu: mỗi {interval} (id {id})"],
    ["loop stopped", "循环已停止", "循環已停止", "ループ停止", "루프 중지됨", "bucle detenido", "boucle arrêtée", "Loop gestoppt", "loop parado", "цикл остановлен", "लूप रुका", "loop dihentikan", "หยุดลูปแล้ว", "döngü durduruldu", "đã dừng vòng lặp"],
    ["schedule added: {spec} (id {id})", "计划已添加：{spec}（id {id}）", "計劃已新增：{spec}（id {id}）", "スケジュール追加：{spec}（id {id}）", "일정 추가됨: {spec} (id {id})", "programación añadida: {spec} (id {id})", "planification ajoutée : {spec} (id {id})", "Zeitplan hinzugefügt: {spec} (id {id})", "agendamento adicionado: {spec} (id {id})", "расписание добавлено: {spec} (id {id})", "शेड्यूल जोड़ा गया: {spec} (id {id})", "jadwal ditambahkan: {spec} (id {id})", "เพิ่มกำหนดการแล้ว: {spec} (id {id})", "zamanlama eklendi: {spec} (id {id})", "đã thêm lịch: {spec} (id {id})"],
    ["scheduler: stopped after 3 consecutive failures", "调度器：连续失败 3 次已停止", "排程器：連續失敗 3 次已停止", "スケジューラ：3 回連続失敗のため停止", "스케줄러: 3회 연속 실패로 중지됨", "planificador: detenido tras 3 fallos consecutivos", "planificateur : arrêté après 3 échecs consécutifs", "Scheduler: nach 3 aufeinanderfolgenden Fehlern gestoppt", "agendador: parado após 3 falhas consecutivas", "планировщик: остановлен после 3 подряд сбоев", "शेड्यूलर: लगातार 3 विफलताओं के बाद रुका", "penjadwal: dihentikan setelah 3 kegagalan berturut-turut", "ตัวจัดกำหนดการ: หยุดหลังล้มเหลวติดต่อกัน 3 ครั้ง", "zamanlayıcı: art arda 3 hatadan sonra durduruldu", "bộ lập lịch: đã dừng sau 3 lần lỗi liên tiếp"],
];

/// Chinese-only overlay for newer UI strings that have not gone through the
/// full 15-language bulk translation pass yet. Non-Chinese languages continue
/// to fall back to English for these keys.
#[rustfmt::skip]
const ZH_ONLY: &[(&str, &str, &str)] = &[
    ("Switch the display currency for cost (USD, CNY, EUR, …)", "切换费用显示货币（USD、CNY、EUR 等）", "切換費用顯示貨幣（USD、CNY、EUR 等）"),
    ("Drive OpenPencil (e.g. /op status, /op generate <prompt>)", "驱动 OpenPencil（例如 /op status、/op generate <prompt>）", "驅動 OpenPencil（例如 /op status、/op generate <prompt>）"),
    ("Browser control panel and commands (e.g. /browser, /browser status)", "浏览器控制面板和命令（例如 /browser、/browser status）", "瀏覽器控制面板和命令（例如 /browser、/browser status）"),
    ("Desktop control status and CDP attach (e.g. /desktop status)", "桌面控制状态与 CDP 附加（例如 /desktop status）", "桌面控制狀態與 CDP 附加（例如 /desktop status）"),
    ("Show the agent team roster (e.g. /team, /team board, /team dismiss <name>)", "显示 agent 团队名册（例如 /team、/team board、/team dismiss <name>）", "顯示 agent 團隊名冊（例如 /team、/team board、/team dismiss <name>）"),
    ("Run a prompt on a recurring interval (this session)", "按固定间隔重复运行提示词（仅限本会话）", "按固定間隔重複執行提示詞（僅限本工作階段）"),
    ("Run a prompt at scheduled times (persisted)", "在指定时间运行提示词（持久化）", "在指定時間執行提示詞（持久化）"),
    ("Show background-turn watchdog health", "显示后台回合看门狗健康状态", "顯示背景回合看門狗健康狀態"),
    ("Background shells, turns, and watchdog health", "后台 Shell、回合与看门狗健康状态", "背景 Shell、回合與看門狗健康狀態"),
    ("Manage Noema long-term memory", "管理 Noema 长期记忆", "管理 Noema 長期記憶"),
    ("Open the sub-agent activity panel", "打开子代理活动面板", "開啟子代理活動面板"),
    ("(clear with /goal clear)", "（用 /goal clear 清除）", "（用 /goal clear 清除）"),
    ("(interrupted)", "（已中断）", "（已中斷）"),
    ("(no hooks configured)", "（未配置 hooks）", "（未設定 hooks）"),
    ("(no skills loaded)", "（未加载 skills）", "（未載入 skills）"),
    ("Browser", "浏览器", "瀏覽器"),
    ("agent created", "代理已创建", "代理已建立"),
    ("agent deleted", "代理已删除", "代理已刪除"),
    ("assemble failed", "组装失败", "組裝失敗"),
    ("attached", "已附加", "已附加"),
    ("attached image from clipboard", "已从剪贴板附加图片", "已從剪貼簿附加圖片"),
    ("auto-compact paused after repeated failures — run /compact to retry manually, or /clear to start fresh", "自动压缩因连续失败已暂停 — 运行 /compact 手动重试，或用 /clear 重新开始", "自動壓縮因連續失敗已暫停 — 執行 /compact 手動重試，或用 /clear 重新開始"),
    ("auto-compact paused: compaction is no longer shrinking the context — run /compact to retry manually, or /clear to start fresh", "自动压缩已暂停：压缩不再缩小上下文 — 运行 /compact 手动重试，或用 /clear 重新开始", "自動壓縮已暫停：壓縮不再縮小上下文 — 執行 /compact 手動重試，或用 /clear 重新開始"),
    ("autonomous orchestration", "自主编排", "自主編排"),
    ("autonomous orchestration: OFF", "自主编排：关闭", "自主編排：關閉"),
    ("autonomous orchestration: ON — the agent may decompose tasks, spawn sub-agents, and define new ones", "自主编排：开启 — 代理可拆解任务、生成子代理并定义新代理", "自主編排：開啟 — 代理可拆解任務、產生子代理並定義新代理"),
    ("available", "可用", "可用"),
    ("bad session path", "会话路径无效", "會話路徑無效"),
    ("busy — finish or interrupt the current turn before /compact", "忙碌中 — 完成或中断当前回合后再运行 /compact", "忙碌中 — 完成或中斷目前回合後再執行 /compact"),
    ("busy — finish or interrupt the current turn first", "忙碌中 — 请先完成或中断当前回合", "忙碌中 — 請先完成或中斷目前回合"),
    ("calling op", "正在调用 op", "正在呼叫 op"),
    ("can't change plugins during a turn — Ctrl+C first", "回合进行中不能更改插件 — 请先按 Ctrl+C", "回合進行中不能變更外掛 — 請先按 Ctrl+C"),
    ("can't clear during a turn — Ctrl+C first", "回合进行中不能清空 — 请先按 Ctrl+C", "回合進行中不能清空 — 請先按 Ctrl+C"),
    ("can't clear the goal text during a turn — run /goal clear again when idle", "回合进行中不能清除目标文本 — 空闲后再运行 /goal clear", "回合進行中不能清除目標文字 — 閒置後再執行 /goal clear"),
    ("can't set goal during a turn — Ctrl+C first", "回合进行中不能设置目标 — 请先按 Ctrl+C", "回合進行中不能設定目標 — 請先按 Ctrl+C"),
    ("can't switch during a turn — Ctrl+C first", "回合进行中不能切换 — 请先按 Ctrl+C", "回合進行中不能切換 — 請先按 Ctrl+C"),
    ("can't switch provider during a turn - Ctrl+C first", "回合进行中不能切换服务商 - 请先按 Ctrl+C", "回合進行中不能切換服務商 - 請先按 Ctrl+C"),
    ("compact failed", "压缩失败", "壓縮失敗"),
    ("compacting the conversation…", "正在压缩对话…", "正在壓縮對話…"),
    ("configure /vision provider <name> first", "请先配置 /vision provider <name>", "請先設定 /vision provider <name>"),
    ("copied input selection to clipboard", "已将输入选区复制到剪贴板", "已將輸入選區複製到剪貼簿"),
    ("copied selection to clipboard", "已将选区复制到剪贴板", "已將選區複製到剪貼簿"),
    ("copy failed", "复制失败", "複製失敗"),
    ("create failed", "创建失败", "建立失敗"),
    ("currency", "货币", "貨幣"),
    ("currency set", "货币已设置", "貨幣已設定"),
    ("current goal", "当前目标", "目前目標"),
    ("current provider does not declare image support; set supportsImages=true or configure /vision provider <name>", "当前服务商未声明支持图像；请设置 supportsImages=true 或配置 /vision provider <name>", "目前服務商未宣告支援圖像；請設定 supportsImages=true 或設定 /vision provider <name>"),
    ("delete failed", "删除失败", "刪除失敗"),
    ("disabled", "已禁用", "已停用"),
    ("effort reset to medium (default)", "推理强度已重置为 medium（默认）", "推理強度已重設為 medium（預設）"),
    ("effort set", "推理强度已设置", "推理強度已設定"),
    ("enabled", "已启用", "已啟用"),
    ("esc to apply", "按 Esc 应用", "按 Esc 套用"),
    ("export failed", "导出失败", "匯出失敗"),
    ("export path escapes the workspace — use an absolute path to export elsewhere", "导出路径超出工作区 — 如需导出到其他位置请使用绝对路径", "匯出路徑超出工作區 — 如需匯出到其他位置請使用絕對路徑"),
    ("exported conversation to", "对话已导出到", "對話已匯出到"),
    ("generating design", "正在生成设计", "正在產生設計"),
    ("goal cleared", "目标已清除", "目標已清除"),
    ("goal set", "目标已设置", "目標已設定"),
    ("goal-loop: no progress (no tool use) for several turns — paused (send a message to resume)", "目标循环：多个回合没有进展（未使用工具）— 已暂停（发送消息可恢复）", "目標循環：多個回合沒有進展（未使用工具）— 已暫停（傳送訊息可恢復）"),
    ("goal-loop: reached the turn cap — paused (send a message to resume)", "目标循环：已达到回合上限 — 已暂停（发送消息可恢复）", "目標循環：已達到回合上限 — 已暫停（傳送訊息可恢復）"),
    ("goal-loop: started", "目标循环：已启动", "目標循環：已啟動"),
    ("images", "图片", "圖片"),
    ("is built-in (not deletable)", "是内置项（不可删除）", "是內建項（不可刪除）"),
    ("language", "语言", "語言"),
    ("load config failed", "加载配置失败", "載入設定失敗"),
    ("load failed", "加载失败", "載入失敗"),
    ("loaded", "已加载", "已載入"),
    ("mode", "模式", "模式"),
    ("no file to view for this image", "此图片没有可查看的文件", "此圖片沒有可檢視的檔案"),
    ("no goal set — use /goal <text> to set one", "未设置目标 — 使用 /goal <text> 设置", "未設定目標 — 使用 /goal <text> 設定"),
    ("no named providers configured", "未配置具名服务商", "未設定具名服務商"),
    ("no image-capable providers configured", "未配置支持图像的服务商", "未設定支援圖像的服務商"),
    ("no image-capable providers configured — add a vision model under `providers` or set supportsImages=true, then pick it here", "未配置支持图像的服务商 — 请在 `providers` 下添加视觉模型或设置 supportsImages=true，然后在此选择", "未設定支援圖像的服務商 — 請在 `providers` 下新增視覺模型或設定 supportsImages=true，然後在此選擇"),
    ("no provider '{name}' in config", "配置中没有服务商 '{name}'", "設定中沒有服務商 '{name}'"),
    ("no saved sessions yet", "暂无已保存会话", "尚無已儲存會話"),
    ("not found", "未找到", "未找到"),
    ("nothing to copy in selection", "选区中没有可复制内容", "選區中沒有可複製內容"),
    ("off", "关闭", "關閉"),
    ("on", "开启", "開啟"),
    ("opening image…", "正在打开图片…", "正在開啟圖片…"),
    ("paste failed", "粘贴失败", "貼上失敗"),
    ("paste image failed", "粘贴图片失败", "貼上圖片失敗"),
    ("plan mode: OFF — full tools restored", "计划模式：关闭 — 已恢复完整工具", "計畫模式：關閉 — 已恢復完整工具"),
    ("plan mode: ON — read-only tools only; research and present a plan, then /plan to execute", "计划模式：开启 — 仅限只读工具；先调研并提出计划，然后用 /plan 执行", "計畫模式：開啟 — 僅限唯讀工具；先調研並提出計畫，然後用 /plan 執行"),
    ("plugin update failed", "插件更新失败", "外掛更新失敗"),
    ("plugins updated", "插件已更新", "外掛已更新"),
    ("press Esc again to clear the input", "再次按 Esc 清空输入", "再次按 Esc 清空輸入"),
    ("provider", "服务商", "服務商"),
    ("queued ({n}) — sends when the turn finishes (Esc to interrupt now)", "已排队（{n}）— 当前回合结束后发送（按 Esc 可立即中断）", "已排隊（{n}）— 目前回合結束後傳送（按 Esc 可立即中斷）"),
    ("queued message removed", "已移除排队消息", "已移除排隊訊息"),
    ("queued message updated", "已更新排队消息", "已更新排隊訊息"),
    ("reload failed", "重新加载失败", "重新載入失敗"),
    ("reloaded skills", "已重新加载 skills", "已重新載入 skills"),
    ("reloaded — tools, MCP, skills, and LSP re-discovered", "已重新加载 — 已重新发现 tools、MCP、skills 和 LSP", "已重新載入 — 已重新探索 tools、MCP、skills 和 LSP"),
    ("removed attached image", "已移除附加图片", "已移除附加圖片"),
    ("run", "运行", "執行"),
    ("running workflow", "正在运行工作流", "正在執行工作流"),
    ("save config failed", "保存配置失败", "儲存設定失敗"),
    ("session deleted", "会话已删除", "會話已刪除"),
    ("sidebar", "侧边栏", "側邊欄"),
    ("steered into the running turn — the agent will see it next step", "已转入正在运行的回合 — 代理会在下一步看到", "已導入正在執行的回合 — 代理會在下一步看到"),
    ("switch failed", "切换失败", "切換失敗"),
    ("thinking output", "思考输出", "思考輸出"),
    ("tool details", "工具详情", "工具詳情"),
    ("try /help", "试试 /help", "試試 /help"),
    ("turn failed", "回合失败", "回合失敗"),
    ("unavailable on this host (need sandbox-exec / bwrap)", "此主机不可用（需要 sandbox-exec / bwrap）", "此主機不可用（需要 sandbox-exec / bwrap）"),
    ("unknown command", "未知命令", "未知命令"),
    ("unknown theme", "未知主题", "未知主題"),
    ("usage: /effort low|medium|high", "用法：/effort low|medium|high", "用法：/effort low|medium|high"),
    ("usage: /vision [mode|provider|prompt|reset]", "用法：/vision [mode|provider|prompt|reset]", "用法：/vision [mode|provider|prompt|reset]"),
    ("usage: /vision mode auto|direct|vision-model", "用法：/vision mode auto|direct|vision-model", "用法：/vision mode auto|direct|vision-model"),
    ("usage: /vision prompt <text>", "用法：/vision prompt <text>", "用法：/vision prompt <text>"),
    ("use /currency <code>", "使用 /currency <code>", "使用 /currency <code>"),
    ("view failed", "查看失败", "檢視失敗"),
    ("vision config reset", "图像理解配置已重置", "圖像理解設定已重設"),
    ("vision mode", "图像理解模式", "圖像理解模式"),
    ("vision model disabled (image mode → auto)", "视觉模型已禁用（图像模式 → auto）", "視覺模型已停用（圖像模式 → auto）"),
    ("vision prompt updated", "视觉提示词已更新", "視覺提示詞已更新"),
    ("vision provider", "视觉服务商", "視覺服務商"),
    ("vision provider '{provider_name}' does not declare image support; set supportsImages=true on it (or its models entry) in config.json", "视觉服务商 '{provider_name}' 未声明支持图像；请在 config.json 中为该服务商（或其 models 条目）设置 supportsImages=true", "視覺服務商 '{provider_name}' 未宣告支援圖像；請在 config.json 中為該服務商（或其 models 條目）設定 supportsImages=true"),
    ("vision provider '{provider_name}' is not configured", "未配置视觉服务商 '{provider_name}'", "未設定視覺服務商 '{provider_name}'"),
    ("vision provider failed", "视觉服务商失败", "視覺服務商失敗"),
    ("vision providers", "视觉服务商", "視覺服務商"),
    ("workflow deleted", "工作流已删除", "工作流已刪除"),
    ("yolo: OFF — tools prompt for approval", "yolo：关闭 — 工具会请求批准", "yolo：關閉 — 工具會請求批准"),
    ("yolo: ON — tools auto-approve (deny rules still apply)", "yolo：开启 — 工具自动批准（deny 规则仍生效）", "yolo：開啟 — 工具自動批准（deny 規則仍生效）"),
    ("✓ goal complete — auto-loop stopped", "✓ 目标完成 — 自动循环已停止", "✓ 目標完成 — 自動循環已停止"),
];

/// The 15 supported UI languages (OpenPencil set).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
    ZhTw,
    Ja,
    Ko,
    Es,
    Fr,
    De,
    Pt,
    Ru,
    Hi,
    Id,
    Th,
    Tr,
    Vi,
}

impl Lang {
    /// Every language, in the canonical order (English first).
    pub const ALL: [Lang; 15] = [
        Lang::En,
        Lang::Zh,
        Lang::ZhTw,
        Lang::Ja,
        Lang::Ko,
        Lang::Es,
        Lang::Fr,
        Lang::De,
        Lang::Pt,
        Lang::Ru,
        Lang::Hi,
        Lang::Id,
        Lang::Th,
        Lang::Tr,
        Lang::Vi,
    ];

    /// BCP-47-ish code used in config (`language: "zh-tw"`).
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Zh => "zh",
            Lang::ZhTw => "zh-tw",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
            Lang::Es => "es",
            Lang::Fr => "fr",
            Lang::De => "de",
            Lang::Pt => "pt",
            Lang::Ru => "ru",
            Lang::Hi => "hi",
            Lang::Id => "id",
            Lang::Th => "th",
            Lang::Tr => "tr",
            Lang::Vi => "vi",
        }
    }

    /// Endonym for the language picker (shown in the language's own script).
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Zh => "简体中文",
            Lang::ZhTw => "繁體中文",
            Lang::Ja => "日本語",
            Lang::Ko => "한국어",
            Lang::Es => "Español",
            Lang::Fr => "Français",
            Lang::De => "Deutsch",
            Lang::Pt => "Português",
            Lang::Ru => "Русский",
            Lang::Hi => "हिन्दी",
            Lang::Id => "Bahasa Indonesia",
            Lang::Th => "ไทย",
            Lang::Tr => "Türkçe",
            Lang::Vi => "Tiếng Việt",
        }
    }

    /// Parse a config code (case-insensitive; `zh_TW`/`zh-tw` both work).
    pub fn from_code(code: &str) -> Option<Lang> {
        let c = code.trim().to_ascii_lowercase().replace('_', "-");
        match c.as_str() {
            "zh-cn" | "zh-hans" | "zh-sg" => return Some(Lang::Zh),
            "zh-hant" | "zh-hk" | "zh-mo" => return Some(Lang::ZhTw),
            _ => {}
        }
        Lang::ALL.into_iter().find(|l| l.code() == c)
    }

    /// Row index into `LOCALES` (the 14 non-English languages). `None` for En,
    /// which needs no lookup (it's the source).
    fn locale_row(self) -> Option<usize> {
        match self {
            Lang::En => None,
            // Order matches LOCALES rows below.
            other => Lang::ALL.iter().skip(1).position(|l| *l == other),
        }
    }
}

static CURRENT: RwLock<Lang> = RwLock::new(Lang::En);

/// Set the active UI language (from the Settings language picker / config).
pub fn set_language(lang: Lang) {
    if let Ok(mut g) = CURRENT.write() {
        *g = lang;
    }
}

/// Set the active language by code (e.g. "zh-tw"). Returns false for an
/// unknown code (the language is left unchanged).
pub fn set_language_code(code: &str) -> bool {
    match Lang::from_code(code) {
        Some(l) => {
            set_language(l);
            true
        }
        None => false,
    }
}

/// The active UI language.
pub fn current() -> Lang {
    CURRENT.read().map(|g| *g).unwrap_or(Lang::En)
}

/// Translate an English UI string into the active language. Unknown strings
/// and English itself pass through unchanged.
pub fn t(s: &'static str) -> &'static str {
    let lang = current();
    // Hand-maintained overlay first (new UI strings), then the generated table.
    if let Some(row) = EXTRA.iter().find(|r| r[0] == s) {
        let col = Lang::ALL.iter().position(|l| *l == lang).unwrap_or(0);
        return row[col];
    }
    if matches!(lang, Lang::Zh | Lang::ZhTw) {
        if let Some(row) = ZH_ONLY.iter().find(|r| r.0 == s) {
            return if lang == Lang::Zh { row.1 } else { row.2 };
        }
    }
    let Some(row) = lang.locale_row() else {
        return s; // English (or an unmapped variant) → source string.
    };
    match SOURCE_STRINGS.iter().position(|src| *src == s) {
        Some(col) => LOCALES[row][col],
        None => s, // Not a translated string → leave as-is.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    fn codes_round_trip() {
        for l in Lang::ALL {
            assert_eq!(Lang::from_code(l.code()), Some(l));
        }
        assert_eq!(Lang::from_code("zh_TW"), Some(Lang::ZhTw));
        assert_eq!(Lang::from_code("klingon"), None);
    }

    #[test]
    #[serial]
    fn zh_cn_alias_enables_recent_command_translations() {
        set_language(Lang::En);
        assert!(set_language_code("zh-CN"));
        assert_eq!(current(), Lang::Zh);
        assert_eq!(
            t("Switch the display currency for cost (USD, CNY, EUR, …)"),
            "切换费用显示货币（USD、CNY、EUR 等）"
        );
        assert_eq!(t("Manage Noema long-term memory"), "管理 Noema 长期记忆");
        assert_eq!(t("Open the sub-agent activity panel"), "打开子代理活动面板");
        set_language(Lang::En);
    }

    #[test]
    fn locale_table_dimensions_match() {
        // 14 non-English rows, each aligned to SOURCE_STRINGS.
        assert_eq!(LOCALES.len(), 14);
        for row in LOCALES.iter() {
            assert_eq!(row.len(), SOURCE_STRINGS.len());
        }
    }

    #[test]
    #[serial]
    fn english_passes_through_and_unknown_is_identity() {
        set_language(Lang::En);
        assert_eq!(t("Settings"), "Settings");
        set_language(Lang::Zh);
        assert_eq!(
            t("a string that is not in the table"),
            "a string that is not in the table"
        );
        set_language(Lang::En);
    }

    #[test]
    #[serial]
    fn overlay_translates_new_keys() {
        set_language(Lang::Zh);
        assert_eq!(t("model"), "模型");
        assert_eq!(t("Configured"), "已配置");
        assert_eq!(t("max output"), "最大输出");
        set_language(Lang::En);
        assert_eq!(t("model"), "model"); // English column = passthrough
        assert_eq!(t("Configured"), "Configured");
    }

    #[test]
    fn overlay_rows_have_no_duplicate_keys() {
        for (i, a) in EXTRA.iter().enumerate() {
            for b in EXTRA.iter().skip(i + 1) {
                assert_ne!(a[0], b[0], "duplicate overlay key {:?}", a[0]);
            }
        }
    }

    #[test]
    fn overlay_keys_do_not_shadow_generated_table() {
        // An EXTRA key that also exists in SOURCE_STRINGS would silently override
        // the generated translation everywhere it's used — forbid it.
        for row in EXTRA {
            assert!(
                !SOURCE_STRINGS.contains(&row[0]),
                "overlay key {:?} shadows the generated table; remove it from EXTRA",
                row[0]
            );
        }
    }

    #[test]
    fn zh_only_rows_have_no_duplicate_or_shadowed_keys() {
        for (i, row) in ZH_ONLY.iter().enumerate() {
            assert_ne!(row.0, row.1, "missing simplified Chinese for {:?}", row.0);
            assert_ne!(row.0, row.2, "missing traditional Chinese for {:?}", row.0);
            assert!(
                !SOURCE_STRINGS.contains(&row.0),
                "ZH_ONLY key {:?} shadows the generated table",
                row.0
            );
            assert!(
                !EXTRA.iter().any(|extra| extra[0] == row.0),
                "ZH_ONLY key {:?} shadows the full overlay",
                row.0
            );
            for other in ZH_ONLY.iter().skip(i + 1) {
                assert_ne!(row.0, other.0, "duplicate ZH_ONLY key {:?}", row.0);
            }
        }
    }

    #[test]
    #[serial]
    fn known_string_translates_when_language_set() {
        // "Settings" is in SOURCE_STRINGS; in zh it must differ from English.
        set_language(Lang::Zh);
        assert_ne!(t("Settings"), "Settings");
        set_language(Lang::En);
    }

    #[test]
    #[serial]
    fn builtin_command_descriptions_translate_to_chinese() {
        set_language(Lang::Zh);
        for command in crate::commands::builtin::BUILTINS {
            assert_ne!(
                t(command.description),
                command.description,
                "/{} description is not translated",
                command.name
            );
        }
        set_language(Lang::En);
    }

    #[test]
    #[serial]
    fn recent_toast_and_status_strings_translate_to_chinese() {
        set_language(Lang::Zh);
        for key in [
            "queued message removed",
            "can't switch during a turn — Ctrl+C first",
            "plugins updated",
            "steered into the running turn — the agent will see it next step",
            "current provider does not declare image support; set supportsImages=true or configure /vision provider <name>",
            "vision provider '{provider_name}' is not configured",
            "goal-loop: started",
            "press Esc again to clear the input",
        ] {
            assert_ne!(t(key), key, "{key:?} is not translated");
        }
        set_language(Lang::En);
    }
}
