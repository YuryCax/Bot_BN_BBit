# 📘 ТЕХНИЧЕСКОЕ ЗАДАНИЕ (ТЗ)
## Low-Latency Алготрейдинговая Система: Binance Futures → Bybit Spot/Perpetual
**Версия:** 1.8  
**Дата:** 08.07.2026  
**Язык разработки:** Rust 1.78+  
**Среда исполнения:** Linux (Ubuntu 22.04/24.04 LTS), `x86_64`/`aarch64`  
**Архитектура:** Trading bot — Rust (`Observer` → `Executor`); **Analyst** — отдельный сервис (§8.6); MVP mono-node (§2.4).

> **Changelog v1.8:** дорожная карта **Фаза 1 / Фаза 2** (§10.6); §8.6.9 **Proposal & Apply** (Telegram/Panel, one-click); §8.7 **Order Book DB** для паттернов; Analyst — фаза 2.  
> **Changelog v1.7:** §8.6 сервис **`analyst`** — offline ИИ-аналитик (не трейдер): наблюдение MA, прогноз направления, отчёты; §9.1 go/no-go; trade journal export.  
> **Changelog v1.6:** исправлен `D_min_net` (fees без ×Lev); §5.5 единый `effective_SL`; fee-BE унифицирован; `MICRO_OK` по Bybit; Safe-Mode поэтапный; cancel-all → auto re-place exchange stop; replay с delay 150 ms; MVP mono-node; futures-first для spot.  
> **Changelog v1.5:** цель «рост депозита, не слив»; §5.4 адаптивное SL/TP по метрикам Binance; §6.0 раздельные депозиты Spot/Futures; §6.3 fee-aware sizing (вход/TP с учётом комиссий); §8.5.9 раздельная остановка торговли и снятие ордеров Spot/Futures.  
> **Changelog v1.4:** добавлена §8.5 «Панель управления» — веб-UI для распределения капитала по парам (% от депозита), включения/остановки пар, отображения профита (realized/unrealized, по паре и суммарно).  
> **Changelog v1.3:** формализовано принятие решения о входе (Observer only), исправлена машина состояний SL/TP, уточнена стратегия (lead-lag/momentum), политика Spot Short, единицы измерения, Risk Engine hot/warm path, freshness 150 ms, exit-EMA по Bybit, fail-safe Spot, политика потерь UDP, `packet_version = 2`.  
> **Changelog v1.2:** добавлен динамический Take-Profit (§5.4) с трейлингом, частичной фиксацией и интеграцией со стоп-лоссом.  
> **Changelog v1.1:** исправлена синхронизация времени между узлами, устранены противоречия в структурах данных и конфиге, интегрированы модули §11, уточнена логика Long/Short и Spot/Futures.

---

## 1. Общие положения и цели системы

| Параметр | Спецификация |
|----------|--------------|
| **Назначение** | Автоматизированная **кросс-биржевая импульсная торговля (momentum / correlated follow-through)** с минимальной задержкой. **Binance Futures** — единственный источник сигналов и метрик для SL/TP; **Bybit** — исполнение. Basis filter (§4.2) отсекает расхождения > 0.05%; система ставит на **следующее** коррелированное движение Bybit вслед за импульсом Binance (не классический lag-arb и не арбитраж уже открытого gap). |
| **Главная цель** | **Рост депозита, а не его слив.** Бот входит только когда ожидаемая прибыль **после комиссий** (§6.3) положительна; в открытой позиции **поднимает SL и TP** по метрикам Binance (§5.4), фиксируя прибыль и защищая капитал. Сделки «в ноль минус комиссия» запрещены. |
| **Приоритет рынков** | **Futures-first:** основной edge из‑за меньших комиссий и Long/Short. Spot — **фаза 2**, только после paper PF ≥ 1.3 на futures (§6.0, §9.1). |
| **Принцип разделения** | `Observer` (Токио) = сбор данных, фильтрация шума, расчёт метрик, **полная оценка условий входа §7**, генерация `MarketStatePacket` с `entry_valid` + `direction_bias`.<br>`Executor` (Сингапур) = freshness/dedup, **Risk Engine (только фильтры исполнения)**, маршрутизация Spot/Futures, исполнение, ведение позиции, **локальный EMA для exit-триггеров**, управление капиталом.<br>**Дублирование entry-логики (Z, D_exp, D_min) на Executor запрещено.** |
| **Технологический стек** | **Trading (Rust):** `tokio`, `simd-json`, `zenoh`, `postcard`, `crossbeam`, `tracing`, `prometheus`, `rustls`, `teloxide`, Control Panel (`axum`).<br>**Analyst (§8.6, отдельный сервис):** Python 3.11+ / TypeScript, LLM API (OpenAI / Anthropic / local), `pandas`, cron/systemd. **Analyst не входит в Rust binary.** |
| **Инфраструктура** | **Production:** AWS `ap-northeast-1` (Observer) + `ap-southeast-1` (Executor), VPC Peering.<br>**MVP (§2.4):** один сервер `ap-southeast-1`, in-process Observer+Executor — до подтверждения PF на paper. |
| **Допустимые инструменты** | Whitelist: 20–35 пар. Стартовый набор — `config.toml` / `symbols.toml`. **Добавление и остановка пар в runtime** — только через Панель управления (§8.5) с hot-reload; произвольная подписка без оператора запрещена. |
| **Ключевые ограничения** | One-way latency Токио→Сингапур P95 ≤ 80 мс, P99 ≤ 110 мс. Freshness drop > 150 ms. Hot path Risk Engine ≤ 10 мкс (§4.2). Проскальзывание входа ≤ 0.05%. Максимальный дневной DD: Spot ≤ 2%, Futures ≤ 1.5%. |
| **Этапы разработки** | **Фаза 1:** боты + Panel (§10.6). **Фаза 2:** БД стаканов + Analyst + Apply с телефона (§8.6–8.7). Фаза 2 **не блокирует** запуск торговли. |

### 1.1. Архитектурная схема

| Узел | Регион AWS | Роль | Ключевые задачи |
|------|------------|------|-----------------|
| **Observer** | `ap-northeast-1` (Токио) | Data & Signal | Binance Futures WS → парсинг → метрики → **entry decision §7** → `MarketStatePacket` → Zenoh |
| **Executor** | `ap-southeast-1` (Сингапур) | Execution & Risk | Приём пакетов → Risk Engine → Bybit Spot / Futures → Position Manager |

```
[ Binance Futures WS ]
          ↓ (10–30 ms)
[ Server A: Observer ] — AWS Tokyo (ap-northeast-1)
   • simd-json parser
   • RingBuffer + Welford/Z/EMA/ATR
   • Entry Engine (§7) → entry_valid + direction_bias
   • Zenoh Publisher (UDP)
          ↓ (50–80 ms P95 via AWS Backbone)
[ Server B: Executor ] — AWS Singapore (ap-southeast-1)
   • Zenoh Subscriber + Freshness Check (≤150 ms)
   • Risk Engine hot path (<10μs) + warm cache
   • Router → BybitSpotConnector / BybitFuturesConnector
   • Bybit V5 Private WS Execution
   • Position Manager (SL/TP state machine §5, Bybit EMA exits)
          ↓
[ Bybit API ] → Spot / USDT Perpetual (Long / Short*)
```
*Short на Spot — только при `spot_margin_enabled = true` (§4.3).

**Ключевые принципы:**
1. **Токио не знает про ордера** — не хранит позиции, баланс, статус исполнения.
2. **Сингапур не пересчитывает entry-метрики** — использует `entry_valid`, `direction_bias`, `d_exp`, `d_min` из пакета; локально считает только **Bybit EMA** для exit-триггеров (§5.3).
3. **Spot и Futures разделены на уровне коннекторов и депозитов** (§6.0): отдельные кошельки Bybit, отдельные лимиты капитала и команды остановки; маршрутизация через `symbols.toml`.
4. **Long и Short** — симметричная логика входа/выхода с инверсией условий (§7).

### 1.2. Принятие решения о входе (единственный источник — Observer)

```
[Tick Binance] → Noise Filter → Metrics (Z, Vel, EMA, ATR, regime)
       ↓
  Entry Engine (§7): D_exp, D_min_net, Z_threshold, regime matrix
       ↓
  entry_valid = 1  ∧  direction_bias ∈ {-1, +1}  →  публикация пакета
  иначе            →  entry_valid = 0, direction_bias = 0
       ↓
[Executor] Freshness + Dedup + Risk Engine (§4.2)
       ↓
  entry_valid = 1  ∧  все risk-флаги OK  →  open_position(direction_bias)
  иначе            →  RISK_SKIP / drop
```

**Executor не вызывает формулы §7 для входа.** Поля `d_exp`, `d_min`, `sigma` в пакете — для аудита, логов и метрик, не для пересчёта.

### 1.3. Логическая цепочка (Binance → Bybit)

```
НАБЛЮДЕНИЕ     АНАЛИЗ + РЕШЕНИЕ          ДЕЙСТВИЕ              СОПРОВОЖДЕНИЕ
(Binance WS)   (Observer)               (Executor → Bybit)    (Executor)
     │              │                          │                    │
  aggTrade      Z, Vel, EMA, ATR         Risk Engine           SL/TP §5.4→§5.5
  bookTicker    Entry Engine §7          open / close          (метрики Binance)
  depth         entry_valid              Spot / Futures        + триггеры Bybit §5.2
                direction_bias
                     │
                     └── MarketStatePacket ──→ (freshness ≤150 ms)
```

**Правило:** Binance — **единственный источник решения о входе** и **адаптации SL/TP в позиции**. Bybit — **единственный источник цены исполнения** (mid, spread, depth) и **microstructure filter** для входа (`MICRO_OK`, §4.2).

### 1.4. Единицы измерения (обязательны для реализации)

| Величина | Единица | Пример |
|----------|---------|--------|
| **Цены** | USDT, `f64` | `67234.50` |
| **PnL** | Доля от entry (`pnl_pct`) | `0.005` = +0.5% |
| **Spread** | Доля от mid (`spread_pct`) | `0.0001` = 0.01% |
| **Velocity** | Доля цены **в секунду** | `(P_now − P_100ms) / P_100ms / 0.1`; `0.0001` ≈ 0.01%/s |
| **ATR, σ** | Абсолютные USDT или доля (`atr_pct = atr / price`) | ATR filter: `atr / price < 0.002` |
| **Z-Score** | Безразмерный | `2.5` |
| **Funding rate** | Доля | `0.0001` = 0.01% |
| **Latency** | Наносекунды UTC wall-clock | `150_000_000` = 150 ms |

---

## 2. Сетевая топология и требования к инфраструктуре

### 2.1. Размещение и соединения
- **Сервер A (Observer):** AWS `ap-northeast-1`. Instance: `c7a.xlarge` (4 vCPU, 8 GB RAM). Private Subnet.
- **Сервер B (Executor):** AWS `ap-southeast-1`. Instance: `c7a.xlarge`. Private Subnet.
- **Сетевой мост:** `AWS VPC Peering` или `Transit Gateway`. Трафик между узлами идёт исключительно по внутренним IP через магистральную сеть AWS. Выход в публичный интернет разрешён только для API бирж, NTP, Telegram, Prometheus, Email.
- **Реальные метрики one-way latency** (`utc_now_ns() − packet.ts_ns` на Executor): P95: 50–80 мс, P99: 90–110 мс, Jitter: ≤ 5 мс.

### 2.2. Протокол межсерверного обмена
- **Библиотека:** `zenoh` v1.0+
- **Транспорт:** UDP (порт 7447), без гарантии доставки; **обязателен `seq_num`** в каждом пакете для детекции потерь и дедупликации.
- **Сериализация:** `postcard` + **версия схемы** `packet_version: u8` (текущая = **`2`**). При изменении структуры — инкремент версии; узлы с несовместимой версией не стартуют.
- **Топик:** `market/binance/{symbol_id}`
- **Частота публикации:** 50–100 Гц в штатном режиме, до 500 Гц при `|Z| ≥ Z_threshold`.
- **Heartbeat:** Отдельный топик `system/heartbeat/tokyo`, пакет с `ts_ns` каждые 100 ms. Пропуски → поэтапный Safe-Mode (§5.2.1); emergency при timeout > 500 ms.
- **Таймстампы:** `ts_ns` = **UTC wall-clock** (`CLOCK_REALTIME`, наносекунды с эпохи). Синхронизация: `chrony` со stratum ≤ 2 на обоих узлах. **Запрещено** использовать `CLOCK_MONOTONIC` в межузловых пакетах.
- **Политика потерь UDP:**
  - **Dedup:** `seq_num <= last_seq_num[symbol_id]` → drop.
  - **Gap detection:** `seq_num > last + 1` → `seq_gap_count++`, лог `WARN`.
  - **Gap storm:** если `seq_gap_count > 10` за 1 с по символу → `pause_entries[symbol_id]` на 5 с, алерт.
  - Потерянные пакеты **не интерполируются**; следующий валидный пакет принимается как есть.
- **Safe-Mode RTT:** скользящий P95 one-way latency за 10 с > **150 ms** → Safe-Mode фаза 1 (§5.2.1). Измерение — `utc_now_ns() − packet.ts_ns`.

### 2.3. Настройки ОС и ядра (Linux Tuning)

**Observer (Токио):**
```bash
sysctl -w net.core.rmem_max=16777216
sysctl -w net.core.wmem_max=16777216
sysctl -w net.ipv4.tcp_timestamps=0
echo performance | tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor
taskset -c 0,1,2,3 ./observer   # все ядра c7a.xlarge под Observer
```

**Executor (Сингапур):**
```bash
# те же sysctl и governor
taskset -c 0,1,2,3 ./executor   # все ядра c7a.xlarge под Executor
```

**Общие настройки сокетов (оба узла):**
```bash
TCP_NODELAY=1 для всех внешних подключений (Binance/Bybit)
SO_RCVBUF/SO_SNDBUF=131072 для Binance/Bybit сокетов
```

### 2.4. MVP mono-node (обязательный этап перед dual-node production)

До развёртывания двух регионов — **обязательный** прогон на одном сервере:

| Параметр | MVP | Production |
|----------|-----|------------|
| **Регион** | `ap-southeast-1` (рядом с Bybit) | Tokyo + Singapore |
| **Процессы** | `bot-mvp` = Observer + Executor in-process (channel вместо Zenoh UDP) | `observer` + `executor` |
| **Inter-node latency** | 0 ms (shared memory / channel) | 50–110 ms |
| **Пары** | 3–5 futures (BTC, ETH, SOL + опц.) | 20–35 |
| **Spot** | **Отключён** (`spot_enabled = false`) | После paper PF ≥ 1.3 futures |
| **Критерий перехода** | Replay PF ≥ 1.2 **с симулированным** delay 150 ms (§9.1) | Live staged 7 дней |

> MVP **не заменяет** dual-node latency-тест; перед production обязателен replay/backtest с `injected_latency_ms = 150`.

---

## 3. Модуль Observer (Токио) — Сбор, парсинг, математика, Entry Engine

### 3.1. Подключения к Binance Futures
- Endpoint: `wss://fstream.binance.com/ws` (Paper: `wss://stream.binancefuture.com/ws`)
- Потоки на пару: `@aggTrade`, `@bookTicker`, `@depth10@100ms` (для оценки ликвидности и imbalance)
- Коннекторы: 4–6 независимых WebSocket-соединений. Каждое подписывается на 5–8 пар из Whitelist.
- Каждое соединение привязано к отдельному CPU-ядру через `tokio::runtime::Builder::on_thread_start`.
- Reconnect: Exponential backoff (1s → 2s → 4s → max 30s), восстановление RingBuffer из последних сохранённых тиков.

> **Примечание:** Binance **Spot** не используется как источник данных. Сигналы генерируются по фьючерсному рынку Binance; исполнение — на Bybit Spot или Perpetual по маппингу в `symbols.toml`.

### 3.2. Парсинг и фильтрация шума
- Библиотека: `simd-json` (AVX2/NEON оптимизация)
- Partial Deserialization: Извлечение только полей `p` (price), `q` (quantity), `T` (trade time), `m` (isBuyerMaker), `b`/`a` (bookTicker).
- Zero-Copy: Данные передаются из сетевого буфера напрямую в аналитический контекст без `Vec::clone` или `String::from`.
- **Фильтры:**
  - `Volume Threshold`: `trade.volume_usd < $10,000` → игнор.
  - `Spread Validation`: Обновление цены принимается только если подтверждено `@aggTrade` или изменением `bookTicker` за < 50 мс.
  - `Clock Sync`: Сверка `Binance.event_time` (UTC ms) с локальным `CLOCK_REALTIME`. Отклонение > 50 мс → лог `WARN`, коррекция offset в `exchange_clock_offset_ns`.

### 3.3. Математическое ядро и Entry Engine
- **RingBuffer:** `Box<[f64; 2048]>` на пару. Lock-free запись/чтение (`crossbeam-queue` или атомарный индекс). Размер: ~16 КБ на пару.
- **Инкрементальный Z-Score:** Алгоритм Welford. $O(1)$ обновление среднего и дисперсии. Окно: 1000 тиков.
- **Velocity:** `(P_now − P_100ms_ago) / P_100ms_ago / 0.1` — **доля/с** (§1.3). Фильтр затухания: если $\frac{d^2P}{dt^2} < 0$, `d_exp` умножается на 0.7 перед сравнением с `D_min`.
- **Triple EMA:** EMA(50), EMA(200), EMA(500) по тикам. В пакет передаются `ema_50` и `ema_200`; EMA(500) используется локально для regime filter.
- **ATR(14):** Инкрементальный расчёт на тиках. Фильтр микро-флета: `ATR / price < 0.2%` → `entry_valid = 0`.
- **Regime Detection:** `TrendStrength = |EMA_50 - EMA_200| / ATR`. `regime: u8` (0=Range, 1=Transition, 2=Trend) — см. матрицу §7.
- **Микроструктурные метрики:** `bid_ask_imbalance`, `volume_delta_100ms` — из `@bookTicker` + `@aggTrade`; передаются в пакете **только для аудита**. Фильтр `MICRO_OK` на входе — по **Bybit** (§4.2 warm path), не по Binance delta.
- **Z_threshold (приоритет):**
  - `use_dynamic_thresholds = false` → `z_score_entry` из config (по умолчанию 2.5).
  - `use_dynamic_thresholds = true` → `Z_threshold = clamp(percentile(|Z|, 5000, 0.95) × 1.1, 1.8, 3.2)`; **`z_score_entry` игнорируется**.
- **Entry Engine (после метрик):** вычисляет `D_min_net`, `D_exp`, проверяет условия §7; выставляет `entry_valid`, `direction_bias`, записывает `d_exp`, `d_min`, `sigma`, `z_threshold_used` в пакет (`d_min` = `D_min_net`).
- **Публикация:** `MarketStatePacket` → `postcard` → Zenoh. Batching ≤ 1 мс.

---

## 4. Модуль Executor (Сингапур) — Маршрутизация, исполнение, риск

### 4.1. Приём и валидация
- Async Zenoh Subscriber, неблокирующий `Stream`.
- Мгновенная десериализация в `MarketStatePacket`.
- **Freshness Check:**
  ```rust
  let latency_ns = utc_now_ns() - packet.ts_ns;
  if latency_ns > 150_000_000 { drop(packet); }  // > 150 ms (P99-safe)
  ```
  `utc_now_ns()` — UTC wall-clock на Executor, синхронизированный через chrony.
- **Dedup и gap:** см. §2.2.
- **Entry gate:** `entry_valid == 0` → пакет используется только для Position Manager (открытые позиции), **новый вход запрещён**.
- Протухшие пакеты игнорируются, лог `INFO`.

### 4.2. Risk Engine (hot path ≤ 10 мкс + warm cache)

Risk Engine разделён на два контура:

| Контур | SLA | Содержимое | Обновление |
|--------|-----|------------|------------|
| **Hot path** | ≤ 10 мкс | `entry_valid`, freshness OK, dedup OK, `RiskFlags` bitmap (precomputed) | Каждый пакет |
| **Warm path** | ≤ 1 ms (данные ≤ 100 ms stale OK) | capital, DD, correlation, Bybit spread/depth, funding, basis | Фоновые задачи |

**Warm path — фоновые задачи (не в hot path):**
- `@bookTicker` + `@aggTrade` Bybit WS → spread, depth, `bybit_mid`, **`bybit_volume_delta_100ms`** (каждый тик).
- `GET /v5/market/tickers?category=linear` → funding (интервал `ticker_poll_interval_sec`, по умолчанию 60 с).
- Balance / margin / DD — каждые 500 ms или по событию fill.

**RiskFlags bitmap (atomic u64, обновляется warm path):**

| Флаг | Условие блокировки входа |
|------|--------------------------|
| `CAPITAL_OK` | Достаточно капитала/маржи в слоте |
| `DD_OK` | DD Spot < 2%, Futures < 1.5% |
| `CORR_OK` | Суммарная экспозиция BTC+ETH одного направления ≤ 60% одного слота; при открытом BTC — ETH того же направления ≤ 30% слота |
| `BOOK_OK` | Bybit spread ≤ 0.01%, depth_10 ≥ $50k |
| `FUNDING_OK` (Futures) | `\|fundingRate\| ≤ 0.0001` или вход не против фандинга |
| `BASIS_OK` | `\|ref_price − bybit_mid\| / bybit_mid ≤ 0.0005` |
| `MICRO_OK` | Long: `bybit_volume_delta_100ms ≥ 0`; Short: `bybit_volume_delta_100ms ≤ 0` (**Bybit** aggTrade/book, warm path) |
| `SPOT_SIDE_OK` | Short на Spot только при `spot_margin_enabled` (§4.3) |
| `PAUSE_OK` | Нет `pause_entries` после gap storm (§2.2) |
| `PAIR_ENABLED` | Пара `enabled = true` в symbols.toml / панели (§8.5.3) |
| `ENTRIES_SPOT_OK` | `halt_entries_spot = false` (§8.5.9) |
| `ENTRIES_FUTURES_OK` | `halt_entries_futures = false` (§8.5.9) |
| `FEE_EDGE_OK` | `D_exp ≥ D_min_net` из пакета (§6.3) |

**Hot path pseudocode:**
```rust
if packet.entry_valid == 0 { return Skip; }
if !risk_flags.all_required() { log RISK_SKIP; return Skip; }
route_and_open(packet.direction_bias);
```

### 4.3. Маршрутизация, Spot Short и коннекторы
```rust
pub trait ExchangeConnector: Send + Sync {
    async fn open_position(&self, pkt: &MarketStatePacket, risk: RiskParams) -> Result<OrderResult, BotError>;
    async fn close_position(&self, pos_id: &str, reason: ExitReason) -> Result<(), BotError>;
    async fn update_stop(&self, pos_id: &str, new_price: f64) -> Result<(), BotError>;
    fn available_capital(&self) -> f64;
    fn instrument_type(&self) -> InstrumentType;
}

pub enum Side { Long, Short }
pub enum InstrumentType { Spot, Futures }
```

- Реализации: `BybitSpotConnector`, `BybitFuturesConnector`.
- Маппинг: `symbols.toml` → `{ instrument, leverage?, spot_margin_enabled? }`.

**Политика Spot Short:**

| Режим | `spot_margin_enabled` | Short на Spot |
|-------|----------------------|---------------|
| **По умолчанию** | `false` | **Запрещён.** `direction_bias = −1` для spot-символа → `SPOT_SIDE_OK = 0`, `RISK_SKIP`. |
| **Spot Margin** | `true` | Разрешён через Bybit Spot Margin (`/v5/spot-margin-trade/*`): borrow base → sell → repay on close. Требует отдельных API-permissions. |

- **Long (Spot/Futures):** `side = Buy`.
- **Short (Futures):** `side = Sell`, open short.
- **Short (Spot Margin):** borrow + sell base; close = buy + repay.

### 4.4. Логика исполнения
- Протокол: `Bybit V5 Private WebSocket`.
- **Pre-allocated order templates:** JSON-буфер и каноническая строка для HMAC pre-allocated at startup. **В момент ордера** (≤ 50 μs): вставка `timestamp_ms`, `symbol`, `side`, `qty` → пересчёт SHA256-HMAC. Полная pre-sign **невозможна** из-за timestamp.
- **Limit IOC + Fallback (по умолчанию):**
  1. `Limit IOC` по цене `mid ± 0.01%`.
  2. Таймер 50 мс. Не исполнился → `Market`.
  3. Проскальзывание `> 0.05%` → слот −20%, алерт.
- Конфиг-флаг `use_limit_fallback: false` → чистый Market.
- Подтверждение: `execution_report` по WS. Статус позиции обновляется в `PositionState`.

### 4.5. Fail-safe Spot (защита при сбое процесса)
- **При открытии позиции (Spot Long):** если `spot_exchange_stop = true` (config), выставляется **reduce-only Stop-Limit** на бирже на уровне `initial_sl × (1 − sl_exchange_buffer_pct)` (Long) с буфером 0.1%.
- **При каждом обновлении virtual SL:** если SL сдвинулся > 0.05% — amend exchange stop (debounce 1 s).
- **При штатном закрытии:** cancel exchange stop до market/limit close.
- **После `cancel-all orders` (§8.5.9):** если spot-позиция открыта → **немедленно** re-place exchange stop по текущему `effective_SL` (§5.5); алерт «stop restored».
- **Crash / SIGKILL:** exchange stop остаётся на бирже — единственная защита Spot при падении Executor.
- **Watchdog:** `systemd` `WatchdogSec=30`, `Restart=always`. При недоступности > 30 s + открытые позиции → Alertmanager `CRITICAL` (оператор вручную через `/flush` или биржу).

---

## 5. Управление позициями и логика выхода

### 5.0. Единая машина состояний SL / TP

Все пороги — `pnl_pct` от entry (§1.3). Обновление — каждый `MarketStatePacket` + `Bybit @bookTicker` mid для PnL и SL/TP trigger.

```
[OPEN]
  SL = Entry ∓ ATR×1.8          (initial)
  TP_fixed = Entry × (1 ± max(initial_target_pct, D_min_net))  (TP-0, §6.3)
       │
  PnL ≥ 0.15% ──→ SL-1: Entry ∓ ATR×0.5  (но не ниже fee-BE, §6.3)
       │
  PnL ≥ 0.30% ──→ SL-BE: Stop = fee-BE (Entry + round-trip fees)
       │           TP phase → TrailArm (начало трейлинга остатка)
       │
  Price crosses TP_fixed ──→ TP-0: закрыть 50% (partial_close_pct)
       │           SL остаётся fee-BE на остатке (§6.3)
       │           TP phase → DynamicTrail
       │
  DynamicTrail ──→ TP = Price ∓ ATR×K_tp (monotonic)
       │           Пересечение TP → закрыть остаток
       │
  [Optional TP-Extended] regime=Trend ∧ |Z|>2.0 (Futures only)
       └── K_tp × 1.5; выход также по Exhaustion / EMA Cross
```

| Порог PnL | SL | TP phase | Действие |
|-----------|-----|----------|----------|
| Open | Entry ∓ ATR×1.8 | Initial | TP_fixed установлен |
| ≥ 0.15% | Entry ∓ ATR×0.5 | Initial | — |
| ≥ 0.30% | fee-BE (§6.3) | TrailArm | Трейлинг TP активен, partial ещё нет |
| Cross TP_fixed | fee-BE (§6.3) | DynamicTrail | Partial 50%; SL/TP на остатке |
| Extended (opt.) | fee-BE или trail | Extended | Futures + Trend only |

**Long:** SL только повышается; TP trail — `max(TP_old, Price − ATR×K_tp)`.  
**Short:** SL только понижается; TP trail — `min(TP_old, Price + ATR×K_tp)`.

**Итоговый SL всегда через §5.5 (`effective_SL`)** — fee-BE, пороги PnL и Binance-adaptive не могут ослабить защиту.

### 5.5. Единое разрешение SL (`effective_SL`)

Три независимых источника предлагают уровень стопа; применяется **наиболее защитный** (для Long — максимальная цена SL):

```
sl_pnl      = SL из порогов PnL (§5.0: initial → SL-1 → fee-BE)
sl_binance  = SL из метрик Binance (§5.4: adaptive tighten)
sl_fee_be   = fee-BE (§6.3) — абсолютный пол

effective_SL_long  = max(sl_pnl, sl_binance, sl_fee_be)
effective_SL_short = min(sl_pnl, sl_binance, sl_fee_be)
```

- **Long:** SL только ↑ относительно предыдущего `effective_SL`.
- **Short:** SL только ↓.
- `current_stop` в `PositionState` = `effective_SL` после каждого пакета Observer + tick Bybit.
- Exchange stop (Spot §4.5, Futures §5.1) синхронизируется с `effective_SL`, не с промежуточными кандидатами.

### 5.1. Виртуальный стоп и синхронизация с биржей
- Стоп по умолчанию **виртуальный** (в памяти Executor).
- **Futures:** реальный `Stop-Market` на Bybit обязателен; amend каждые 5 с или при `ΔATR > 15%`. Ликвидационная цена локально; `Distance_to_Liq < 0.4%` → закрытие 50%.
- **Spot:** виртуальный SL + optional exchange Stop-Limit (§4.5). Safe-Mode — см. §5.2 (поэтапный, не мгновенный close).

### 5.2. Триггеры выхода

| Триггер | Условие | Источник данных | Действие |
|---------|---------|-----------------|----------|
| Stop-Loss | `bybit_mid` пересекает `effective_SL` (§5.5) | Bybit bookTicker | Market (Limit fallback §4.4) |
| Take-Profit | `bybit_mid` пересекает `current_tp` | Bybit bookTicker | Partial / full (§5.0) |
| Exhaustion | `\|Vel_binance\| < 0.00005` ∧ `\|Z_binance\| < 0.5` | Пакет Observer | Market close |
| EMA Cross | `bybit_ema_50` пересекает `bybit_ema_200` против позиции | **Executor local EMA on Bybit mid** | Market close |
| Spread Expansion | Bybit spread > 0.01% ∨ depth_10 < $50k | Bybit bookTicker | Limit ±0.02% |
| Safe-Mode | Heartbeat loss / P95 latency > 150 ms | §2.2, §5.2.1 | Поэтапно: halt → close |

**Приоритет:** Safe-Mode → Stop-Loss → Take-Profit → EMA Cross → Exhaustion → Spread Expansion.

#### 5.2.1. Safe-Mode (поэтапный, без panic-close на первом glitch)

| Фаза | Условие | Действие |
|------|---------|----------|
| **1 — Caution** | 1 пропуск heartbeat ИЛИ P95 latency > 150 ms | `halt_entries` spot + futures; позиции ведутся по SL/TP |
| **2 — Defensive** | 2–3 пропуска heartbeat подряд (`safe_mode_heartbeat_misses`, default 3) | Tighten all SL → `effective_SL = fee-BE` minimum |
| **3 — Emergency** | 5 пропусков ИЛИ heartbeat timeout > 500 ms | Market close **всех** позиций spot + futures |

> Фаза 1 не закрывает позиции — защита от ложных срабатываний на сетевом jitter.

> **Важно:** Entry-метрики (Z, Vel, EMA Binance) — из пакета Observer. **Exit EMA Cross** — только по Bybit mid на Executor (инкрементальный EMA(50/200), отдельный от Observer). Это **не дублирование entry-логики**.

### 5.3. Динамический Take-Profit (конфиг §10.1)

Take-Profit виртуальный; исполнение программное (§4.4). Фазы соответствуют §5.0:

| Фаза (`tp_phase`) | Код | Описание |
|-------------------|-----|----------|
| **Initial** | 0 | TP_fixed = Entry × (1 ± `initial_target_pct`) |
| **TrailArm** | 1 | PnL ≥ `trail_arm_pct` (0.3%): трейлинг активен, partial ещё нет |
| **DynamicTrail** | 2 | После partial close: TP = Price ∓ ATR × K_tp |
| **Extended** | 3 | Futures + Trend: K_tp × 1.5; Spot — **отключено** |

**Формулы:**
```
TP_fixed_long   = Entry × (1 + initial_target_pct)    // default 0.5%
TP_fixed_short  = Entry × (1 − initial_target_pct)

TP_trail_long   = CurrentPrice − ATR × K_tp_trail
TP_trail_short  = CurrentPrice + ATR × K_tp_trail

K_tp_trail = clamp(base_tp_trail_atr × (1 + 0.1 × |Z|), 0.8, 1.5)
```

**Связь TP ↔ SL (согласовано с §5.0):**

| Событие | SL | TP |
|---------|-----|-----|
| PnL ≥ 0.15% | SL-1 (Entry ∓ ATR×0.5), ≥ fee-BE | Initial |
| PnL ≥ 0.30% | fee-BE (§6.3) | TrailArm |
| TP-0 partial 50% | fee-BE на остатке | DynamicTrail |
| SL до TP-0 | Full close | Cancelled |
| `take_profit.enabled = false` | §5.0 SL only | Disabled |

### 5.4. Адаптивное поднятие SL/TP по метрикам Binance

**Смысл:** пока позиция открыта, Observer продолжает слать `MarketStatePacket` с актуальными Z, Velocity, ATR, regime с Binance Futures. Position Manager на Executor **на каждом пакете** пересчитывает SL/TP — не ждёт фиксированных порогов PnL, а реагирует на **положение дел на Binance**.

| Сигнал Binance (из пакета) | Действие по SL | Действие по TP |
|----------------------------|----------------|----------------|
| `\|Z\|` растёт, `\|Vel\|` в сторону позиции, regime=Trend | Поднять SL (Long) / опустить SL (Short) на `ΔATR × k_sl_tight` (k=0.3); не ниже fee-BE (§6.3) | Расширить TP trail: `K_tp × 1.2`; фаза → Extended при `\|Z\| > 2.0` |
| `\|Z\|` падает, `\|Vel\|` затухает (exhaustion) | Удержать текущий SL (не ослаблять) | Сузить TP trail; при `\|Vel\| < 0.00005` — досрочный выход (§5.2) |
| Regime → Range после Trend | SL → fee-BE минимум | Partial close 50% если PnL ≥ `D_min_net`; остаток — tight trail |
| Импульс против позиции (`Vel` разворот) | Немедленно SL → fee-BE или market close если PnL < 0 | Отменить Extended; ускорить выход |
| ATR вырос > 15% vs entry | Пересчитать SL distance = `ATR × 1.8` (не уже fee-BE) | Пересчитать TP trail distance |

**Правила монотонности (защита депозита):**
- **Long:** SL только ↑; TP trail только ↑ (не отдаём заработанное).
- **Short:** SL только ↓; TP trail только ↓.
- Поднятие SL **никогда** не опускает защиту ниже **fee-breakeven** (цена входа + round-trip комиссии, §6.3).
- Если Binance импульс сильный (`\|Z\| ≥ 2.5` ∧ `\|Vel\| > velocity_min`) — `sl_binance` → fee-BE **до** порога PnL 0.30% (опережающая защита); итог через §5.5.

**Источник ATR/Z/Vel/regime:** пакет Observer (Binance). **Trigger price:** Bybit mid (§5.2). **Итоговый SL:** §5.5.

---

## 6. Управление капиталом и мультиинструментальность

### 6.0. Раздельные депозиты Spot и Futures (Bybit)

**Да — у Spot и Futures на Bybit разные балансы.** Даже в Unified Trading Account (UTA) API возвращает **раздельный equity**:
- **Spot wallet** — USDT для спотовых сделок (`/v5/account/wallet-balance`, `accountType=UNIFIED`, coin USDT available for spot).
- **Derivatives wallet** — маржа USDT Perpetual (`totalMarginBalance`, `totalAvailableBalance` для linear).

Система **никогда не смешивает** эти пулы при расчёте размера позиции и allocation.

| Параметр | Spot | Futures |
|----------|------|---------|
| Баланс | `spot_equity_usdt` | `futures_equity_usdt` |
| Allocation пары | `spot_alloc_pct` — % от **spot** equity | `futures_alloc_pct` — % от **futures** equity |
| DD лимит | ≤ 2% от spot equity | ≤ 1.5% от futures equity |
| Max open positions | ≤ 3 | ≤ 2 |
| Остановка торговли | `halt_entries_spot` (§8.5.9) | `halt_entries_futures` (§8.5.9) |

- Обновление балансов: warm path Risk Engine, каждые 500 ms (`GET /v5/account/wallet-balance`).
- В панели — **два отдельных блока**: «Spot депозит» и «Futures депозит» с equity, allocation, PnL, кнопками stop/cancel.
- Перевод USDT Spot ↔ Futures **не автоматизируется** ботом; только вручную оператором на бирже (out of scope v1.6).

**Политика Spot (фаза 2):**
- Spot **отключён** в MVP (`deployment.spot_enabled = false`, §2.4).
- Включение spot-пар — только после futures paper PF ≥ `min_futures_pf_for_spot` (default 1.3).
- Spot-пары: `initial_target_pct` ≥ `spot_min_tp_pct` (default **0.008** = 0.8%) — иначе net edge после fees (§6.3) слишком мал при VIP0.
- Spot Long-only по умолчанию (§4.3) — половина Binance Short-сигналов недоступна.

### 6.1. Слоты и размер позиции
- Капитал пары = `wallet_equity × alloc_pct` (spot или futures — §6.0).
- Внутри лимита пары — до 5 слотов (по 20% от лимита пары) для одновременных входов **не используется** в v1.5; **одна позиция на пару**.
- **Формула размера (Futures):** `$Qty = \frac{WalletEquity \times AllocPct \times Risk\%}{ATR \times Multiplier}$`
- **Формула размера (Spot):** та же; `Lev = 1`.
- Келли: **только оффлайн** для калибровки `Risk%` и `Multiplier`.
- Динамическая адаптация: ликвидность недостаточна → объём уменьшается до `slippage ≤ 0.05%`.

### 6.3. Fee-aware sizing — торговля с учётом комиссий (не «кормить биржу»)

**Принцип:** каждая сделка должна иметь **положительное мат. ожидание после всех издержек**. Бот не открывает и не держит позицию, если net edge ≤ 0.

**Комиссии (конфиг `[fees]`, обновляются из Bybit VIP tier или вручную):**

| Рынок | Maker | Taker | По умолчанию (Bybit VIP0) |
|-------|-------|-------|---------------------------|
| Spot | `spot_maker_pct` | `spot_taker_pct` | 0.10% / 0.10% |
| Futures | `futures_maker_pct` | `futures_taker_pct` | 0.02% / 0.055% |

**Round-trip cost** (комиссии — от **notional**, leverage **не умножает** fee %):
```
fee_round_trip_spot    = spot_taker_pct × 2 + slippage_budget
fee_round_trip_futures = futures_taker_pct × 2 + slippage_budget
slippage_budget        = slippage_limit_pct (default 0.05%)
profit_buffer          = fee_profit_buffer_pct (default 0.03%)

D_min_net_spot    = fee_round_trip_spot + profit_buffer      # ≈ 0.28% VIP0
D_min_net_futures = fee_round_trip_futures + profit_buffer   # ≈ 0.19% VIP0
```

> **Исправлено v1.6:** формула `× Lev` для fees **удалена** — комиссия биржи считается от notional сделки, не от маржи.

**Минимальное движение для входа (Observer, §7):**
```
D_min_net = D_min_net_spot | D_min_net_futures   (по instrument пары)
Вход разрешён ⟺ D_exp ≥ D_min_net
```

**Fee-breakeven (безубыток с комиссиями, не «голый» entry):**
```
BE_long  = Entry × (1 + fee_round_trip)
BE_short = Entry × (1 − fee_round_trip)
```
SL-BE (§5.0, §5.4) устанавливается на **BE_long/BE_short**, а не на цену входа — иначе стоп в ноль = убыток на комиссиях.

**Минимальный Take-Profit:**
```
TP_min_long  = Entry × (1 + D_min_net)
TP_min_short = Entry × (1 − D_min_net)
```
`initial_target_pct` (§10.1) должен быть ≥ `D_min_net`; иначе конфиг невалиден при старте.

**В панели профит показывается двумя строками:**
- **Gross PnL** — без комиссий.
- **Net PnL** — минус накопленные fees (realized); **Net — основной показатель** для оценки «депозит растёт или сливается».

### 6.2. Лимиты и ограничения
- `MaxOpenPositions`: Spot ≤ 3, Futures ≤ 2.
- `Daily Drawdown`: Spot ≤ 2%, Futures ≤ 1.5%. При достижении → `stop_all`, алерт, блокировка до ручного сброса.
- **Correlation Limit (единая формулировка):** экспозиция BTC + ETH **одного направления** (Long или Short) ≤ **60% одного слота**. Если BTC Long открыт — новый ETH Long ≤ **30% слота**. Аналогично для Short.
- `Margin Call (Futures)`: `MarginRatio < 0.2` → аварийное закрытие всех фьючерсных позиций.

---

## 7. Формулы и алгоритмы (Observer — Entry Engine)

| Назначение | Формула | Константы / Условия |
|------------|---------|---------------------|
| **Z-Score** | `$Z = \frac{P_{curr} - \mu}{\sigma}$` | `\|Z\| ≥ Z_threshold` (§3.3) |
| **Z_threshold** | dynamic **или** `z_score_entry` | dynamic имеет **приоритет** при `use_dynamic_thresholds = true` |
| **Velocity** | `(P_now − P_100ms) / P_100ms / 0.1` | доля/с; Long: `> velocity_min`; Short: `< −velocity_min` |
| **Мин. движение (Spot)** | `$D_{min\_net} = fee_{round\_trip\_spot} + profit\_buffer$` | §6.3; ≈ 0.28% VIP0 |
| **Мин. движение (Futures)** | `$D_{min\_net} = fee_{round\_trip\_futures} + profit\_buffer$` | §6.3; ≈ 0.19% VIP0; **без ×Lev** |
| **Ожидаемое движение** | `$D_{exp} = \alpha \cdot \|Z\| \cdot \sigma + \beta \cdot \|Vel\| \cdot \Delta t$` | `α=0.4`, `β=0.6`, `Δt=0.3` с |
| **Вход Long** | `D_exp ≥ D_min_net` ∧ `\|Z\| ≥ Z_threshold` ∧ `Vel > velocity_min` ∧ `EMA_50 > EMA_200` | см. regime matrix |
| **Вход Short** | `D_exp ≥ D_min_net` ∧ `\|Z\| ≥ Z_threshold` ∧ `Vel < −velocity_min` ∧ `EMA_50 < EMA_200` | см. regime matrix |
| **Regime matrix** | | |
| — Range (0) | Long ✓, Short ✓ | |
| — Transition (1) | Long ✓, Short ✓ | |
| — Trend (2) | Long только если `EMA_50 > EMA_200` | Short только если `EMA_50 < EMA_200` |
| **Стоп-лосс (initial)** | `$SL = P_{entry} \pm (ATR \times K)$` | `K = 1.8` |
| **TP Fixed** | `$TP = P_{entry} \times (1 \pm TP_{init})$` | `TP_init = 0.005`; partial 50% |
| **TP Trail** | `$TP = P_{curr} \mp ATR \times K_{tp}$` | monotonic; §5.0 |
| **K_tp adaptive** | `$K_{tp} = clamp(K_{base} \times (1 + 0.1 \times \|Z\|), 0.8, 1.5)$` | `K_base = 1.0` |
| **EMA** | `$EMA_t = (P_t - EMA_{t-1}) \times \frac{2}{n+1} + EMA_{t-1}$` | `n ∈ {50, 200, 500}` |
| **Welford** | `σ = sqrt(M2 / (n-1))` | окно 1000 тиков |
| **Regime** | `TrendStrength = \|EMA_50 - EMA_200\| / ATR` | Range < 1.2; Trend > 2.0 |
| **Exhaustion Exit** | `\|Vel\| < 0.00005` ∧ `\|Z\| < 0.5` | Vel из пакета (Binance) |
| **Спред-фильтр (Observer)** | `spread_pct > 0.0001` | pause signal |
| **Волатильность-фильтр** | `σ/price < 0.0005` ∨ `ATR/price < 0.002` | `entry_valid = 0` |

**Результат Entry Engine → пакет:**
- Условия Long → `entry_valid=1`, `direction_bias=+1`
- Условия Short → `entry_valid=1`, `direction_bias=−1`
- Иначе → `entry_valid=0`, `direction_bias=0`

---

## 8. Мониторинг, логирование, безопасность и алерты

### 8.1. Логирование
- Библиотека: `tracing` + `tracing-subscriber` (async file appender).
- Формат `.bin` v1:
  ```
  [8 bytes] timestamp_ns (u64, UTC)
  [2 bytes] event_type (u16): 1=TICK, 2=PACKET, 3=ORDER, 4=FILL, 5=EXIT
  [4 bytes] payload_len (u32)
  [N bytes] payload (postcard)
  ```
- Ротация: 100 МБ или 1 час. Сжатие `zstd` в фоне.

### 8.2. Метрики (Prometheus)
- `latency_p95_ms`, `latency_p99_ms`, `packet_loss_count`, `seq_gap_count`, `pause_entries_active`
- `slippage_avg_pct`, `positions_open_count{type="spot"|"futures"}`
- `tp_hit_count{type="partial"|"full"}`, `tp_trail_distance_pct`, `sl_phase_count`
- `daily_drawdown_pct`, `heartbeat_status{node="tokyo"|"singapore"}`
- `risk_engine_hot_path_us`, `risk_flags_stale_ms`
- Endpoint: `/metrics`, scrape interval: 5s.

### 8.3. Каналы алертов
| Канал | Назначение | SLA | Реализация |
|-------|------------|-----|------------|
| **Telegram** | Оператор: вход/выход, статус, ошибки | < 10 сек | `teloxide` на Executor |
| **Prometheus Alertmanager** | DD > 1.5%, P95 latency > 150ms, gap storm, API disconnect, watchdog timeout | Мгновенно | `stop_all` webhook |
| **Email** | Ежедневный аудит PnL, win-rate, slippage | < 5 мин | SMTP, 00:00 UTC |

### 8.4. Безопасность
- API Keys: AWS KMS или Hashicorp Vault. Дешифровка при старте в RAM.
- IP Whitelist на Binance/Bybit.
- Permissions: trade + read, без вывода. Spot Margin — отдельное разрешение.
- Ротация ключей: раз в 30 дней через CI/CD.
- Конфиг: `.toml` не коммитится; AWS Parameter Store / env.

### 8.5. Панель управления (Control Panel)

Отдельный сервис **`control-panel`** на Executor (Сингапур) или выделенном admin-хосте в той же VPC. Назначение — операторское управление капиталом, парами и мониторинг профита **без правки конфигов вручную на сервере**.

#### 8.5.1. Общие требования

| Параметр | Спецификация |
|----------|--------------|
| **Доступ** | HTTPS (TLS), Basic Auth или JWT; bind только на private IP / VPN / SSH-tunnel. Публичный интернет без mTLS **запрещён**. |
| **Обновление UI** | WebSocket push каждые 1–2 с (позиции, PnL, статус пар); REST — для мутаций. |
| **Hot path** | Панель **не участвует** в hot path Risk Engine и исполнении ордеров. Все команды — async через command queue Executor. |
| **Аудит** | Каждое изменение (allocation, enable/disable pair) → `tracing` event + append-only audit log. |

#### 8.5.2. Распределение капитала по парам (% от депозита Spot / Futures)

**Spot и Futures — разные кошельки (§6.0).** Allocation задаётся **отдельно** для каждого типа счёта.

| Поле | Применимо к | Смысл |
|------|-------------|-------|
| `spot_alloc_pct` | `instrument = "spot"` | % от **spot equity** (USDT на спот-счёте) |
| `futures_alloc_pct` | `instrument = "futures"` | % от **futures equity** (маржа USDT Perpetual) |

- Единица: доля (`0.10` = 10%). Сумма `spot_alloc_pct` по всем enabled spot-парам ≤ `1.0`. Сумма `futures_alloc_pct` по enabled futures-парам ≤ `1.0`. Остаток — резерв на соответствующем кошельке.
- Одна пара = один instrument (spot **или** futures), не оба одновременно.
- Изменение allocation:
  - **Без открытой позиции** — немедленно; warm path ≤ 500 ms.
  - **С открытой позицией** — только для следующего входа.
- Размер позиции: `Qty = WalletEquity × AllocPct × Risk% / (ATR × Multiplier)` (§6.1).
- Валидация: `0 < alloc_pct ≤ 0.25` на пару в рамках своего кошелька.

**Dashboard в UI — два блока:**

```
┌─ SPOT ─────────────────────────────┐  ┌─ FUTURES ──────────────────────────┐
│ Equity: 300 USDT                     │  │ Equity: 200 USDT                   │
│ Allocated: 65%  Free: 35%            │  │ Allocated: 80%  Free: 20%          │
│ Net PnL today: +2.1 USDT (+0.7%)     │  │ Net PnL today: +4.3 USDT (+2.1%)   │
│ [Stop new entries] [Cancel all orders]│  │ [Stop new entries] [Cancel all orders]│
└──────────────────────────────────────┘  └────────────────────────────────────┘
```

**Таблица пар (фрагмент):**

| Пара | Type | Enabled | Alloc % | Wallet | Net PnL | Pos |
|------|------|---------|---------|--------|---------|-----|
| BTCUSDT | Futures | ✓ | 20% | Futures 200 | +1.2% | Long |
| ETHUSDT | Spot | ✓ | 15% | Spot 300 | +0.4% | — |
| SOLUSDT | Futures | ⏸ | 10% | Futures 200 | — | — |

#### 8.5.3. Добавление и остановка пар

| Действие | Поведение |
|----------|-----------|
| **Add pair** | Символ + `instrument` (spot/futures) + `spot_alloc_pct` или `futures_alloc_pct`. Observer: `SubscribeSymbol`. SLA ≤ 30 с. |
| **Stop pair (disable)** | `enabled = false` для пары. **Новые входы** блокируются (`PAUSE_OK`-аналог на уровне пары). Открытая позиция **не закрывается** автоматически — Position Manager продолжает SL/TP. Observer: `UnsubscribeSymbol` после подтверждения «нет позиции» или по явной галочке «force stop + close» (эквивалент `/flush` только для этой пары). |
| **Remove pair** | Только если `enabled = false` и нет позиции. Запись удаляется из runtime-конфига; Observer отписывается. |
| **Лимит whitelist** | 20–35 пар; при попытке добавить сверх лимита — ошибка UI. |

> **Согласование с §3.1:** автоматическая подписка/отписка без оператора по-прежнему запрещена; панель — единственный легитимный канал изменения состава пар.

#### 8.5.4. Отображение профита (Profit)

Панель обязана показывать:

| Метрика | Описание | Обновление |
|---------|----------|------------|
| **Unrealized PnL (net)** | После estimated fees | WebSocket 1–2 с |
| **Realized PnL (session, net)** | Закрытые сделки минус fees | По fill |
| **Realized PnL (today / 24h, net)** | Календарные сутки UTC | REST + WS |
| **Realized PnL (total, net)** | С `initial_spot_deposit` + `initial_futures_deposit` (§10.1) | REST |
| **Gross vs Net** | Обе колонки; **Net — primary** (§6.3) | REST |
| **Profit by pair** | realized + unrealized net, win-rate, fees, slippage | REST |
| **Equity curve** | Spot и Futures **отдельные** графики | REST |
| **Fees paid** | Spot fees / Futures fees раздельно | REST |

Формат отображения: USDT (абсолют) и **% от депозита** (§1.3, `pnl_pct`). Цветовая индикация: profit ≥ 0 — зелёный; DD > порога §6.2 — красный баннер.

#### 8.5.5. REST API (минимальный контракт)

```
GET  /api/v1/dashboard                    → spot/futures equity, alloc, net_pnl, pairs[]
GET  /api/v1/pairs                        → enabled, spot_alloc_pct | futures_alloc_pct
POST /api/v1/pairs                          → { symbol, instrument, alloc_pct, leverage? }
PATCH /api/v1/pairs/{id}                  → { enabled?, spot_alloc_pct?, futures_alloc_pct? }
DELETE /api/v1/pairs/{id}
GET  /api/v1/profit?period=24h&wallet=spot|futures|all
GET  /api/v1/positions?wallet=spot|futures|all
POST /api/v1/positions/{id}/close
POST /api/v1/trading/halt                 → { wallet: "spot"|"futures"|"all", halt_entries: true }
POST /api/v1/trading/resume               → { wallet: "spot"|"futures"|"all" }
POST /api/v1/orders/cancel-all            → { wallet: "spot"|"futures" }  # снять все открытые ордера
POST /api/v1/positions/close-all          → { wallet: "spot"|"futures", confirm: true }
GET  /api/v1/suggestions?status=pending   → pending Analyst proposals (Фаза 2)
POST /api/v1/suggestions/{id}/apply       → one-click apply
POST /api/v1/suggestions/{id}/reject
WS   /ws/v1/stream                        → + push «N pending suggestions»
```

#### 8.5.6. Конфигурация

```toml
[control_panel]
enabled = true
bind_addr = "127.0.0.1:8080"       # только private; nginx reverse proxy опционально
auth_mode = "jwt"                   # "basic" | "jwt"
jwt_secret_env = "PANEL_JWT_SECRET"
default_spot_alloc_pct = 0.05
default_futures_alloc_pct = 0.05
max_pair_alloc_pct = 0.25
ws_push_interval_ms = 2000
audit_log_path = "/var/log/bot/panel_audit.jsonl"
```

Расширение `symbols.toml`:

```toml
[[symbol]]
id = 2
binance = "ETHUSDT"
bybit = "ETHUSDT"
instrument = "spot"
spot_margin_enabled = false
enabled = true
spot_alloc_pct = 0.15               # только для instrument = "spot"
# futures_alloc_pct = 0.20          # только для instrument = "futures"
```

#### 8.5.7. Метрики Prometheus (дополнение к §8.2)

- `panel_pair_enabled{symbol}` — 0/1
- `panel_spot_alloc_pct{symbol}`, `panel_futures_alloc_pct{symbol}`
- `profit_net_usdt{wallet="spot"|"futures"}`
- `halt_entries_active{wallet="spot"|"futures"}`

#### 8.5.8. Связь с Telegram (§10.3)

Telegram — мобильные алерты; панель — основной UI. `/status` — краткая сводка Spot/Futures отдельно.

#### 8.5.9. Остановка торговли и снятие ордеров (Spot / Futures раздельно)

Оператор должен иметь **независимый контроль** над Spot и Futures.

| Команда (панель / API) | Spot | Futures | Эффект |
|------------------------|------|---------|--------|
| **Stop new entries** | `halt_entries_spot = true` | `halt_entries_futures = true` | Блок новых входов; **открытые позиции** продолжают SL/TP (§5.4) |
| **Resume entries** | `halt_entries_spot = false` | `halt_entries_futures = false` | Снятие блокировки входов |
| **Cancel all orders** | `POST .../orders/cancel-all?wallet=spot` | `...wallet=futures` | Отмена **всех** открытых ордеров на счёте (limit, stop, conditional). Позиции **не закрываются** |
| **Close all positions** (опционально, confirm) | spot only | futures only | Market close всех позиций + cancel orders |
| **Stop ALL** | оба флага halt | | + cancel all orders на обоих счетах |

**Cancel all orders — детали:**
- Spot: `POST /v5/order/cancel-all` (`category=spot`).
- Futures: `POST /v5/order/cancel-all` (`category=linear`).
- Включает: pending limits, conditional. **Exchange stop-loss (§4.5) тоже снимается.**
- **Обязательно после cancel:** если есть открытая spot-позиция → auto **re-place exchange stop** по `effective_SL` (§5.5) в течение 2 с; UI-предупреждение + Telegram «stop restored».
- Futures: после cancel re-place `Stop-Market` по `effective_SL` (§5.1).
- **Не путать** с «stop pair» (§8.5.3): halt блокирует только **новые** входы.

**UI:** две кнопки на каждый кошелёк — «⏸ Stop new bets» и «✕ Cancel all orders»; статус `HALTED` / `ACTIVE` в dashboard.

### 8.6. Analyst Service — ИИ-аналитик (отдельный сервис, **Фаза 2**, не трейдер)

Сервис **`analyst`** — автономный процесс в **Фазе 2** (§10.6). Наблюдает MA, стаканы (§8.7), паттерны; формирует **предложения** (настройки, allocation, ручные ставки). **Исполнение — только после Apply оператором** (§8.6.4), в т.ч. с телефона через Telegram.

> **Analyst не автоторгует.** Нет Apply — нет изменений. Hot path бота Analyst **не трогает**.

#### 8.6.1. Архитектура (Фаза 2)

```
[ Trading Bot — Фаза 1 ]          [ Analyst — Фаза 2 ]           [ Operator (mobile/PC) ]
 Observer + Executor + Panel            │                              │
      │                                  │                              │
      ├── .bin + trades DB ─────────────►│ MA + orderbook patterns      │
      ├── Panel command queue ◄──────────│ SuggestionQueue              │
      │                                  │ LLM forecast                 ├──► Telegram 🔔
      └── POST /suggestions/{id}/apply ◄─┤ Proposal builder             │    [Apply][Reject]
                                         └── TimescaleDB §8.7           └──► Panel «Pending»
```

| Параметр | Спецификация |
|----------|--------------|
| **Deploy** | `analyst.service`; **стартует после** ≥ 14 дней сбора данных Фазы 1 |
| **Runtime** | Python 3.11+ (рекомендуется) |
| **Расписание** | Forecast каждые 15 min; daily report 00:30 UTC; scan pending → push оператору |
| **LLM** | OpenAI / Anthropic / Ollama; timeout 60 s → rule-based fallback |

#### 8.6.2. Наблюдение за MA (Moving Averages)

Analyst строит и отслеживает MA **независимо** от торгового бота:

| MA | Источник | Назначение |
|----|----------|------------|
| **EMA(50), EMA(200), EMA(500)** Binance | `.bin` PACKET; EMA(500) — из TICK или REST klines | Тренд сигнального рынка (§3.3) |
| **EMA(50), EMA(200)** Bybit | `.bin` + пересчёт из Bybit mid | Тренд рынка исполнения |
| **SMA(20), SMA(50)** (опц.) | REST klines 5m Binance + Bybit | Краткосрочный контекст |
| **Cross state** | `EMA50 vs EMA200` на обеих биржах | Golden/Death cross; расхождение Binance↔Bybit |

**Rule-based MA (детерминированно, до LLM):**
```
ma_bias = +1  если EMA50 > EMA200  и  price > EMA50   (uptrend)
ma_bias = −1  если EMA50 < EMA200  и  price < EMA50   (downtrend)
ma_bias =  0  иначе                                    (range/chop)

ma_spread_pct = (EMA50 − EMA200) / price
ma_divergence = sign(ma_bias_binance) ≠ sign(ma_bias_bybit)  → alert
```

#### 8.6.3. Прогноз направления (LLM + MA)

Прогноз на горizont `forecast_horizon_min` (default 30 min) — **не ордер**, а оценка:

**Вход LLM (JSON):** symbol, ts, binance `{price, ema50, ema200, z, vel, regime}`, bybit `{mid, ema50, ema200}`, `ma_bias_*`, `bot_last_signal`, `recent_trades_net_pnl_pct`.

**Выход (обязательная схема):**
```json
{
  "direction_forecast": "up" | "down" | "neutral",
  "confidence": 0.0,
  "horizon_min": 30,
  "ma_summary": "EMA50>EMA200 на обеих биржах",
  "divergence_vs_bot": "aligned" | "contradicts" | "no_bot_signal",
  "risk_note": "",
  "suggestions": []
}
```

**Constraints:** `confidence < 0.55` → `neutral`; forecast ≠ bot → alert.

#### 8.6.4. Proposal & Apply — one-click применение (оператор не at desk)

Analyst создаёт **формализованные предложения** (`Suggestion`), оператор **одной кнопкой** отправляет их в Panel → Executor command queue.

**Типы предложений (`SuggestionKind`):**

| Kind | Что предлагает | После Apply |
|------|----------------|-------------|
| `config_patch` | `{ "signals.z_score_entry": 2.8 }` | hot-reload config |
| `pair_alloc` | `{ symbol, futures_alloc_pct: 0.12 }` | §8.5.2 |
| `pair_enable` / `pair_disable` | вкл/выкл пару | §8.5.3 |
| `manual_entry` | `{ symbol, direction, instrument, size_pct }` | Risk Engine → open (§4.2) |
| `close_position` | `{ position_id, reason }` | Market close |
| `halt_wallet` | `{ wallet: "spot"\|"futures", halt: true }` | §8.5.9 |

**Жизненный цикл:**
```
Analyst → POST /internal/suggestions (status=pending)
       → Push: Telegram + Panel badge «N pending»
Operator → [Apply] или [Reject]
       → Panel POST /api/v1/suggestions/{id}/apply
       → Executor validates + executes (≤2 s)
       → status=applied|rejected|expired; audit log
```

**Telegram (teloxide / Analyst bot):**
```
📊 Analyst · BTCUSDT
Прогноз: UP (conf 72%)
Предложение: manual_entry Long futures, 5% alloc
Обоснование: EMA50>200, Z=2.3, стакан bid-heavy
[✅ Apply] [❌ Reject] [📋 Details]
```
- Callback `apply:{suggestion_id}` / `reject:{suggestion_id}`
- TTL предложения: **24 h** (config `suggestion_ttl_hours`); expired → auto-reject
- **Rate limit Apply:** max 10 applies / час / оператор

**Безопасность Apply:**
- `manual_entry` проходит **полный Risk Engine**; отклоняется если DD/halt/fees fail
- `manual_entry` max size: `min(proposed_size_pct, max_manual_entry_pct)` default **5%** alloc
- `config_patch` — только whitelist ключей (`signals.*`, `take_profit.*`, `risk.*` — не API keys)
- Replay-check (опц.): если `require_replay_on_config = true`, Apply config только если replay PF не падает > 10%

**Panel API (дополнение §8.5.5):**
```
GET  /api/v1/suggestions?status=pending
GET  /api/v1/suggestions/{id}
POST /api/v1/suggestions/{id}/apply      # operator JWT
POST /api/v1/suggestions/{id}/reject
```

**Структура `Suggestion`:**
```json
{
  "id": "uuid",
  "created_at": "ISO8601",
  "expires_at": "ISO8601",
  "status": "pending|applied|rejected|expired",
  "kind": "manual_entry",
  "payload": { "symbol": "BTCUSDT", "direction": "long", "instrument": "futures", "size_pct": 0.05 },
  "rationale": "LLM + MA + orderbook imbalance 0.62",
  "confidence": 0.72,
  "source": "analyst"
}
```

#### 8.6.5. Отчёты и Analyst API

| Канал | Содержимое |
|-------|------------|
| Telegram | Daily digest + **push pending suggestions** |
| Panel | Вкладки «Analyst», «Pending (N)» |
| Trade journal | → TimescaleDB (§8.7) |

```
GET  /analyst/v1/forecasts
GET  /analyst/v1/forecasts/{symbol}
GET  /analyst/v1/reports/daily?date=
POST /analyst/v1/analyze-now
POST /analyst/v1/suggestions/generate    # internal / cron
```

#### 8.6.6. Trade journal → БД

Поля: `entry_ts`, `exit_ts`, `symbol`, `instrument`, `direction`, prices, `z_entry`, `vel_entry`, `d_exp`, `d_min_net`, `regime`, `ma_bias_*`, `ob_imbalance_entry` (§8.7), `slippage_pct`, `fees_usdt`, `net_pnl_pct`, `exit_reason`. Export из Executor 1×/час → PostgreSQL/TimescaleDB.

#### 8.6.7. Конфиг `analyst.toml`

```toml
[analyst]
enabled = false                       # true только в Фазе 2
min_trading_days_before_start = 14    # ждём накопления БД
llm_provider = "openai"
llm_model = "gpt-4o-mini"
forecast_interval_min = 15
suggestion_ttl_hours = 24
max_manual_entry_pct = 0.05
push_pending_to_telegram = true
database_url = "postgres://bot@localhost/bot_warehouse"

[analyst.ma]
ema_periods = [50, 200, 500]
```

#### 8.6.8. Go/No-Go перед live (Фаза 1 — без Analyst)

| # | Критерий | Порог | Блокирует live? |
|---|----------|-------|-----------------|
| 1–4, 6–7 | §8.6.8 v1.7 (PF, follow-through, paper) | см. v1.7 | **Да** |
| 5 | Analyst forecast accuracy | ≥ 55% | **Нет** (Фаза 2) |

#### 8.6.9. Связь с ботом

- Без **Apply** Analyst **не влияет** на торговлю.
- Apply → Panel command queue → тот же путь, что ручные действия оператора (§8.5).
- Auto-apply **запрещён** в v1.8.

### 8.7. Order Book Warehouse — накопление стаканов и паттернов (**Фаза 2**)

Параллельно с Analyst — сбор и хранение данных стакана для offline-анализа и обогащения LLM.

#### 8.7.1. Источники и частота

| Биржа | Поток | Частота | Глубина |
|-------|-------|---------|---------|
| Binance Futures | `@depth10@100ms` | 10 Hz | 10 levels |
| Bybit Linear | `@orderbook.50` или REST snapshot | 10–20 Hz | 50 levels |
| Bybit Spot | `@orderbook.50` | 10 Hz | 50 levels |

**Collector `book-collector`** (Rust или Python sidecar, **не hot path**):
- Подписка на те же пары, что whitelist
- Snapshot + delta → нормализованный формат → batch insert в БД каждые 1–5 с

#### 8.7.2. Схема БД (TimescaleDB / PostgreSQL)

```sql
-- orderbook_snapshots (hypertable по ts)
ts TIMESTAMPTZ, symbol TEXT, exchange TEXT,  -- binance_futures | bybit_linear | bybit_spot
bid_prices DOUBLE PRECISION[], bid_qtys DOUBLE PRECISION[],
ask_prices DOUBLE PRECISION[], ask_qtys DOUBLE PRECISION[],
mid DOUBLE PRECISION, spread_pct DOUBLE PRECISION,
imbalance DOUBLE PRECISION,              -- (bid_vol - ask_vol) / (bid_vol + ask_vol) top-10
depth_bid_usd DOUBLE PRECISION, depth_ask_usd DOUBLE PRECISION

-- orderbook_features (агрегаты 1 min — для Analyst)
ts, symbol, exchange,
imbalance_mean, imbalance_std, spread_mean,
bid_wall_detected BOOL, ask_wall_detected BOOL,
binance_bybit_imbalance_delta DOUBLE PRECISION
```

Retention: raw snapshots **30 дней**; минутные features **1 год**. Сжатие TimescaleDB continuous aggregates.

#### 8.7.3. Паттерны для Analyst

| Паттерн | Описание | Использование |
|---------|----------|---------------|
| **Imbalance momentum** | Δimbalance за 30 s | Подтверждение direction forecast |
| **Wall appearance** | bid/ask wall > 3× median size | `risk_note` в suggestion |
| **Binance→Bybit lag** | imbalance Binance опережает Bybit | Оценка follow-through |
| **Spread widening** | spread > p95 | Предложение halt или skip pair |
| **Pre-signal footprint** | ob shape за 60 s до `entry_valid` | Offline tuning Z/Vel |

Analyst при генерации `manual_entry` / forecast **обязан** прикладывать `ob_imbalance`, `wall_detected` из `orderbook_features`.

#### 8.7.4. Минимальный объём перед включением Analyst

| Метрика | Порог |
|---------|-------|
| Календарных дней сбора | ≥ 14 |
| Snapshots на пару | ≥ 100 000 |
| Закрытых сделок в journal | ≥ 50 |

---

## 9. Тестирование, валидация и развертывание

### 9.1. Тестирование
| Этап | Инструменты | Критерии приемки |
|------|-------------|------------------|
| Unit/Integration | `cargo test`, `mockall`, `proptest` | Coverage ≥ 80% критического пути; 0 критических багов |
| Entry/Risk split | Mock packets | Executor **не** вызывает Z/D_exp; reject при `entry_valid=0` |
| SL/TP state machine | proptest порогов | Monotonic SL/TP; порядок 0.15→0.30→TP-0 |
| Network Simulation | `tc netem` (delay 50ms jitter 5ms loss 0.1%) | Safe-Mode; gap storm pause; ордера не дублируются |
| Replay Engine | Симулятор на `.bin` логах | TCA: slippage, fees, latency; PF ≥ 1.2 для live |
| **Latency replay** | Inject `injected_latency_ms = 150` | **Follow-through rate ≥ 40%**: доля сигналов, где Bybit движется в сторону `direction_bias` в окне 300 ms после delayed entry |
| Paper Trading | Bybit Testnet + Binance Futures Testnet | ≥ 100 сделок **Futures**; Spot — только после futures PF ≥ 1.3; DD < 1% |
| Live Staged | 1% депозита | 7 дней без критических ошибок |
| **Control Panel** | API + UI e2e | Spot/Futures alloc раздельно; halt entries spot ≠ futures; cancel-all orders; net PnL после fees |
| **Fee edge** | Unit + replay | Сделки с `D_exp < D_min_net` не открываются; BE включает fees |
| **Binance-adaptive SL/TP** | Replay + proptest | `effective_SL` монотонен; ≥ fee-BE; §5.5 |
| **Safe-Mode phased** | Integration | Фаза 1 не закрывает; фаза 3 закрывает все |
| **Cancel + restore stop** | Integration | После cancel-all exchange stop восстановлен ≤ 2 s |
| **MVP mono-node** | §2.4 deploy | 3–5 futures pairs; spot disabled; PF paper ≥ 1.2 |
| **Analyst Service** | §8.6.4 | Proposal+Apply e2e; manual_entry через Risk; TTL expire |
| **Order Book DB** | §8.7 | ≥100k snapshots/пара; features 1 min |
| **Go/No-Go** | §8.6.7 checklist | Пункты 1–4, 6–7 pass перед live |

### 9.2. CI/CD и развертывание

**Фаза 1 (минимальный deploy):**
- `bot-mvp.service` (или `observer` + `executor`), `telegram-alerts.service`, `control-panel.service`

**Фаза 2 (добавляется к Фазе 1):**
- `book-collector.service`, `analyst.service`, PostgreSQL/TimescaleDB

- Pipeline: GitHub Actions → fmt → clippy → test → release → S3 → SSH deploy.
- Конфиг: `/etc/bot/config.toml`, `symbols.toml`, `analyst.toml` (Фаза 2).
- Rollback: документирован в Runbook.

---

## 10. Приложения

### 10.1. `config.toml` (единый, без дубликатов секций)
```toml
[capital]
initial_spot_deposit_usdt = 300.0
initial_futures_deposit_usdt = 200.0
risk_per_trade_pct = 0.01

[fees]
spot_maker_pct = 0.001
spot_taker_pct = 0.001
futures_maker_pct = 0.0002
futures_taker_pct = 0.00055
fee_profit_buffer_pct = 0.0003      # мин. edge поверх round-trip fees

[deployment]
mode = "mvp"                          # "mvp" | "production"
spot_enabled = false                  # true только после futures PF ≥ min_futures_pf_for_spot
min_futures_pf_for_spot = 1.3
mvp_futures_pairs = ["BTCUSDT", "ETHUSDT", "SOLUSDT"]

[safe_mode]
heartbeat_miss_caution = 1            # фаза 1: halt entries
heartbeat_miss_defensive = 3          # фаза 2: SL → fee-BE
heartbeat_miss_emergency = 5          # фаза 3: close all
heartbeat_emergency_timeout_ms = 500

[spot]
spot_min_tp_pct = 0.008               # min TP для spot (0.8%); выше futures default

[execution]
default_leverage_futures = 10
max_leverage_futures = 20
margin_mode = "isolated"
slippage_limit_pct = 0.0005
use_limit_fallback = true
limit_fallback_timeout_ms = 50
limit_offset_pct = 0.0001
slippage_adaptive_resize = true

[risk]
max_daily_drawdown_spot = 0.02
max_daily_drawdown_futures = 0.015
correlation_limit_btc_eth = 0.6      # суммарно одного направления
correlation_limit_eth_when_btc_open = 0.3
atr_multiplier_stop = 1.8
atr_min_filter = 0.002

[take_profit]
enabled = true
initial_target_pct = 0.005           # TP-0: +0.5% — partial close
partial_close_pct = 0.5
trail_arm_pct = 0.003                # PnL ≥ 0.3%: TrailArm (до partial)
sl_breakeven_pct = 0.003             # PnL ≥ 0.3%: SL → fee-BE (§6.3)
sl_tighten_pct = 0.0015              # PnL ≥ 0.15%: SL-1
base_tp_trail_atr = 1.0
extended_trend_tp = true             # TP-Extended: только Futures
extended_trend_z_min = 2.0

[signals]
z_score_entry = 2.5                  # игнорируется при use_dynamic_thresholds=true
z_score_exit = 0.5
velocity_min = 0.0001                # 0.01%/s

[noise_filter]
min_trade_volume_usd = 10000.0
max_spread_pct = 0.0001
volatility_stddev_threshold = 0.0005

[network]
max_latency_ms = 150
heartbeat_interval_ms = 100
heartbeat_timeout_ms = 500
safe_mode_latency_p95_ms = 150
packet_version = 2
seq_gap_pause_threshold = 10
seq_gap_pause_duration_sec = 5

[adaptive_strategy]
use_dynamic_thresholds = true
threshold_window_ticks = 5000
threshold_percentile = 0.95
threshold_multiplier = 1.1
threshold_min = 1.8
threshold_max = 3.2

[regime_filter]
mode = "auto"  # "auto" | "range" | "trend"
trend_strength_threshold = 2.0
range_strength_threshold = 1.2

[funding_basis]
enabled = true
max_funding_rate = 0.0001
basis_threshold_pct = 0.0005
ticker_poll_interval_sec = 60

[spot_failsafe]
spot_exchange_stop = true
sl_exchange_buffer_pct = 0.001
stop_amend_debounce_ms = 1000

[replay]
log_version = "v1"
log_rotation_mb = 100
log_rotation_hours = 1
min_replay_pf_for_live = 1.2

[alerts]
telegram_bot_token = "${TELEGRAM_BOT_TOKEN}"
telegram_chat_id = "${TELEGRAM_CHAT_ID}"
prometheus_endpoint = "/metrics"

[control_panel]
enabled = true
bind_addr = "127.0.0.1:8080"
auth_mode = "jwt"
jwt_secret_env = "PANEL_JWT_SECRET"
default_spot_alloc_pct = 0.05
default_futures_alloc_pct = 0.05
max_pair_alloc_pct = 0.25
ws_push_interval_ms = 2000
audit_log_path = "/var/log/bot/panel_audit.jsonl"

[whitelist]
pairs = [
  "BTCUSDT","ETHUSDT","SOLUSDT","BNBUSDT","XRPUSDT","DOGEUSDT",
  "AVAXUSDT","LINKUSDT","ADAUSDT","POLUSDT","DOTUSDT","ARBUSDT",
  "OPUSDT","NEARUSDT","PEPEUSDT","WIFUSDT","FETUSDT","RENDERUSDT",
  "ENAUSDT","SUIUSDT","TIAUSDT","SEIUSDT","JUPUSDT","WLDUSDT","NOTUSDT"
]
```

### 10.2. `symbols.toml` (пример маппинга Spot / Futures)
```toml
[[symbol]]
id = 1
binance = "BTCUSDT"
bybit = "BTCUSDT"
instrument = "futures"
leverage = 10
enabled = true
futures_alloc_pct = 0.20

[[symbol]]
id = 2
binance = "ETHUSDT"
bybit = "ETHUSDT"
instrument = "spot"
spot_margin_enabled = false   # Long only; Short → RISK_SKIP
enabled = true
spot_alloc_pct = 0.15

[[symbol]]
id = 3
binance = "SOLUSDT"
bybit = "SOLUSDT"
instrument = "futures"
leverage = 5
enabled = true
futures_alloc_pct = 0.10

# Пример Spot Margin (опционально):
# [[symbol]]
# id = 4
# binance = "XRPUSDT"
# bybit = "XRPUSDT"
# instrument = "spot"
# spot_margin_enabled = true    # Short через Bybit Spot Margin API
```

### 10.3. Telegram-бот (Executor + Analyst push, Фаза 2)

**Trading (всегда):**
- `/status` → Spot/Futures equity, net PnL, позиции, Latency, halt-флаги
- `/pause spot` / `/pause futures` / `/pause all` → Stop new entries (§8.5.9)
- `/resume spot` / `/resume futures` / `/resume all`
- `/cancel spot` / `/cancel futures` → Cancel all orders
- `/flush spot` / `/flush futures` / `/flush all` → Close all + halt

**Analyst (Фаза 2) — оператор away from desk:**
- Push-сообщение с inline-кнопками **`[✅ Apply]` `[❌ Reject]` `[📋 Details]`** на каждое `Suggestion`
- `/pending` → список активных предложений (id, kind, symbol, TTL)
- `/apply {id}` / `/reject {id}` — текстовый fallback если кнопки недоступны
- Callback `apply:uuid` → Panel `POST /api/v1/suggestions/{id}/apply` от имени оператора

### 10.4. Структуры данных (Rust)
```rust
pub const PACKET_VERSION: u8 = 2;

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
pub struct MarketStatePacket {
    pub packet_version: u8,       // = 2
    pub ts_ns: u64,               // UTC wall-clock
    pub seq_num: u32,
    pub symbol_id: u16,
    pub entry_valid: u8,          // 0 = no entry signal, 1 = valid
    pub direction_bias: i8,       // -1 Short, 0 Neutral, +1 Long (valid if entry_valid=1)
    pub regime: u8,               // 0 Range, 1 Transition, 2 Trend
    pub z_score: f32,
    pub z_threshold_used: f32,    // actual threshold applied (dynamic or fixed)
    pub velocity: f32,            // fraction per second (§1.3)
    pub sigma: f32,               // Welford σ, for audit
    pub d_exp: f32,               // expected move, for audit
    pub d_min: f32,               // min required move, for audit
    pub ema_50: f64,
    pub ema_200: f64,
    pub atr: f32,
    pub ref_price: f64,           // Binance Futures mid — basis check
    pub spread_pct: f32,
    pub volume_usd: f32,
    pub bid_ask_imbalance: f32,
    pub volume_delta_100ms: f32,
}
// ~80 bytes postcard; не использовать #[repr(C, packed)]

pub struct PositionState {
    pub id: String,
    pub symbol_id: u16,
    pub side: Side,
    pub instrument: InstrumentType,
    pub entry_price: f64,
    pub qty: f64,
    pub qty_remaining: f64,
    pub current_stop: f64,
    pub current_tp: f64,
    pub sl_phase: u8,             // 0=Initial, 1=Tight, 2=Breakeven
    pub tp_phase: u8,             // 0=Initial, 1=TrailArm, 2=DynamicTrail, 3=Extended
    pub partial_done: bool,
    pub pnl_pct: f64,
    pub open_time_ns: u64,
    pub exchange_stop_id: Option<String>,  // Spot fail-safe §4.5
}

// Executor-local (не в пакете):
pub struct BybitExitMetrics {
    pub ema_50: f64,
    pub ema_200: f64,
    pub mid: f64,
    pub volume_delta_100ms: f32,   // MICRO_OK filter (§4.2)
}
```

### 10.5. Порядок внедрения (спринты — детализация)

> Сводная дорожная карта по фазам — **§10.6**.

| Спринт | Модуль | Фаза |
|--------|--------|------|
| **0–7** | MVP → Paper → Live staged (§10.6 Фаза 1) | **1** |
| **8** | Control Panel §8.5 | **1** |
| **9a** | `book-collector` + TimescaleDB §8.7 | **2** |
| **9b** | Trade journal → DB; 14 дней накопления | **2** |
| **10** | Analyst: MA + LLM + forecasts | **2** |
| **11** | Proposal & Apply: Telegram + Panel §8.6.4 | **2** |

### 10.6. Дорожная карта разработки (Фаза 1 → Фаза 2)

```
═══════════════════════════════════════════════════════════════════
  ФАЗА 1 — ТОРГОВЛЯ + ПАНЕЛЬ          цель: bot зарабатывает на paper/live
═══════════════════════════════════════════════════════════════════
  Этап 1.1  MVP mono-node (§2.4)
            └── Rust: Observer+Executor in-process, 3 futures pairs

  Этап 1.2  Ядро стратегии
            └── Entry Engine §7, Risk §4.2, SL/TP §5, fee-aware §6.3
            └── .bin логи, Replay + latency 150 ms

  Этап 1.3  Control Panel §8.5
            └── equity, alloc %, halt/cancel, net PnL, pairs CRUD
            └── Telegram trading commands §10.3

  Этап 1.4  Paper → Go/No-Go §8.6.8
            └── PF ≥ 1.2, follow-through ≥ 40%, 100+ сделок
            └── Live staged 1% депозита, 7 дней

  Этап 1.5  Production (опционально)
            └── dual-node Tokyo+Singapore, 20+ pairs, spot off

  ✓ Критерий завершения Фазы 1: live futures торгует; Panel работает;
    operator управляет с телефона (halt/status/flush).

═══════════════════════════════════════════════════════════════════
  ФАЗА 2 — БД + ИИ АНАЛИТИК          цель: паттерны + советы + Apply
═══════════════════════════════════════════════════════════════════
  Этап 2.1  Data Warehouse §8.7
            └── PostgreSQL/TimescaleDB
            └── book-collector: Binance depth + Bybit orderbook
            └── мин. 14 дней сбора, 100k snapshots/пара

  Этап 2.2  Trade journal в БД §8.6.6
            └── связка сделок с ob_imbalance на входе
            └── continuous aggregates (1 min features)

  Этап 2.3  Analyst Service §8.6
            └── MA engine, LLM forecast up/down/neutral
            └── pattern rules: imbalance, walls, lag Binance→Bybit

  Этап 2.4  Proposal & Apply §8.6.4
            └── SuggestionQueue: config, alloc, manual_entry, close
            └── Telegram [Apply][Reject] + Panel «Pending»
            └── manual_entry через Risk Engine; TTL 24h

  ✓ Критерий завершения Фазы 2: Analyst шлёт предложения; operator
    применяет с телефона; audit log; forecast accuracy tracked.

═══════════════════════════════════════════════════════════════════
  OUT OF SCOPE v1.8: auto-apply; Analyst в hot path; spot без PF gate
═══════════════════════════════════════════════════════════════════
```

| Фаза | Срок (ориентир) | Стек | Зависит от |
|------|-----------------|------|------------|
| **1** | 8–12 недель | Rust, axum, teloxide | — |
| **2** | 6–8 недель после старта БД | Python, TimescaleDB, LLM | Фаза 1 live ≥ 14 дней |

**Параллельность:** `book-collector` можно запустить **в конце Фазы 1** (этап 1.4 paper) — к старту Analyst БД уже накоплена.

---

## 11. Исправления v1.0 → v1.1 (справочно)

| # | Проблема v1.0 | Исправление v1.1 |
|---|---------------|------------------|
| 1 | `CLOCK_MONOTONIC` в межузловых пакетах | UTC `CLOCK_REALTIME` + chrony на обоих узлах |
| 2 | Freshness check сравнивал несинхронизированные monotonic часы | `utc_now_ns() - packet.ts_ns` |
| 3 | `taskset` запускал observer и executor на одной машине | Отдельные команды per-server |
| 4 | Basis filter без цены Binance в пакете | Добавлено поле `ref_price` |
| 5 | Funding из `/funding/history` | Текущий rate из `/market/tickers` |
| 6 | Дублирующая секция `[execution]` в config.toml | Объединена в одну |
| 7 | `MarketStatePacket` — три несовместимых определения | Единая структура §10.4 |
| 8 | Postcard «обратно совместим» при добавлении полей | Явный `packet_version`; несовместимость = stop |
| 9 | Z-threshold: 2.0 в §3.3 vs 2.5 в §7 | Единый порог 2.5 (или dynamic) |
| 10 | `D_min` с Leverage для Spot | Отдельные формулы Spot/Futures |
| 11 | Long/Short не формализованы | Явные условия входа §7 |
| 12 | `MATICUSDT` устарел | Заменён на `POLUSDT` |
| 13 | Нет `seq_num` при UDP без guarantee | Добавлен dedup + метрики потерь |
| 14 | `regime` в тексте §11.2, но не в struct | Поле `regime: u8` в пакете |
| 15 | Черновые блоки «для вставки» внутри документа | Интегрированы в основные разделы |
| 16 | Coverage ≥ 95% | 80% критического пути (реалистичнее) |
| 17 | `teloxide` на обоих узлах | Только Executor / alerts service |

---

## 12. Изменения v1.1 → v1.2 (справочно)

| # | Добавлено | Описание |
|---|-----------|----------|
| 1 | §5.4 Dynamic Take-Profit | 4 фазы: Initial → Trail Arm → Dynamic Trail → Extended (Trend) |
| 2 | Частичное закрытие 50% | На TP-0; остаток ведётся трейлингом |
| 3 | Связь TP ↔ SL | TP-0 → SL в безубыток; TP-1 → SL Phase 1 |
| 4 | `[take_profit]` в config.toml | Все параметры конфигурируемы, `enabled = false` для отключения |
| 5 | `PositionState` расширен | `current_tp`, `tp_phase`, `qty_remaining` |
| 6 | Формулы §7 | TP Initial, TP Trail, адаптивный `K_tp` |
| 7 | Метрики Prometheus | `tp_hit_count`, `tp_trail_distance_pct` |
| 8 | Приоритет триггеров §5.3 | Safe-Mode > SL > TP > EMA > Exhaustion |

---

## 13. Изменения v1.2 → v1.3

| # | Проблема v1.2 | Исправление v1.3 |
|---|---------------|------------------|
| 1 | Неясно, кто принимает решение о входе | §1.2: Observer Entry Engine; Executor только Risk (§4.2) |
| 2 | TP-1 (0.3%) раньше TP-0 (0.5%) | §5.0: единая машина 0.15→0.30→TP-0 partial→DynamicTrail |
| 3 | «Арбитраж» vs basis filter | §1: lead-lag/momentum, не классический арбитраж |
| 4 | Spot Short без margin | §4.3: Long-only по умолчанию; margin опционально |
| 5 | Risk Engine ≤ 10 μs нереалистичен целиком | §4.2: hot path + warm cache + RiskFlags bitmap |
| 6 | Freshness 120 ms vs P99 сети | Freshness и Safe-Mode → **150 ms** |
| 7 | `Trend-only-short` не в enum regime | §7: regime matrix 0/1/2 |
| 8 | EMA Cross exit по Binance, позиция на Bybit | §5.2: exit EMA только по Bybit mid на Executor |
| 9 | Spot без биржевого SL при crash | §4.5: optional exchange Stop-Limit + watchdog |
| 10 | Pre-signed templates некорректно | §4.4: pre-allocated buffer + HMAC at order time |
| 11 | Z-threshold dynamic vs fixed | §3.3: dynamic имеет приоритет |
| 12 | Correlation 60% vs 30% ETH | §6.2 + §4.2: единая формулировка |
| 13 | RTT Safe-Mode не определён | §2.2: P95 one-way latency 10 s window |
| 14 | Нет политики UDP gap | §2.2: gap storm → pause 5 s |
| 15 | Нет единиц Velocity/PnL | §1.3: таблица единиц |
| 16 | `packet_version = 1`, не хватало полей | `packet_version = 2`: `entry_valid`, `d_exp`, `d_min`, `sigma`, `z_threshold_used` |

---

## 14. Изменения v1.3 → v1.4

| # | Добавлено | Описание |
|---|-----------|----------|
| 1 | §8.5 Панель управления | Веб-UI + REST/WebSocket API на Executor |
| 2 | `capital_alloc_pct` | Распределение капитала по парам в % от депозита |
| 3 | Enable / disable / add / remove pair | Операторское управление составом whitelist в runtime |
| 4 | Profit dashboard | Realized / unrealized PnL, по парам, equity curve, fees |
| 5 | `[control_panel]` в config.toml | Параметры сервиса панели |
| 6 | `enabled`, `capital_alloc_pct` в symbols.toml | Состояние и лимит капитала на пару |
| 7 | Спринт 8 | Внедрение Control Panel |
| 8 | Whitelist §1 | Runtime-изменение пар только через панель (не произвольная подписка) |

---

## 15. Изменения v1.4 → v1.5

| # | Добавлено / изменено | Описание |
|---|----------------------|----------|
| 1 | **Главная цель** §1 | Рост депозита; SL/TP по Binance; запрет сделок «минус комиссия» |
| 2 | §5.4 | Адаптивное поднятие SL/TP по Z, Vel, ATR, regime с Binance |
| 3 | §6.0 | **Раздельные депозиты** Spot и Futures (разные кошельки Bybit) |
| 4 | §6.3 | Fee-aware sizing: `D_min_net`, fee-BE, net PnL в панели |
| 5 | §8.5.2 | `spot_alloc_pct` / `futures_alloc_pct` — % от **своего** кошелька |
| 6 | §8.5.9 | Stop new entries + cancel all orders — **Spot и Futures раздельно** |
| 7 | `[fees]` config | Комиссии maker/taker для расчёта edge |
| 8 | Risk flags | `ENTRIES_SPOT_OK`, `ENTRIES_FUTURES_OK`, `FEE_EDGE_OK` |
| 9 | API | `/trading/halt`, `/orders/cancel-all` с `wallet=spot\|futures` |

---

## 16. Изменения v1.5 → v1.6

| # | Исправлено / добавлено | Описание |
|---|------------------------|----------|
| 1 | **D_min_net** §6.3, §7 | Убрано ошибочное `× Lev` для комиссий; fees от notional |
| 2 | §5.5 `effective_SL` | `max(sl_pnl, sl_binance, sl_fee_be)` — единый SL |
| 3 | fee-BE | Унифицирован во всех таблицах §5.0, §5.3 (убран «Entry BE») |
| 4 | `MICRO_OK` §4.2 | Фильтр по `bybit_volume_delta_100ms`, не Binance |
| 5 | §5.2.1 Safe-Mode | 3 фазы: halt → tighten SL → emergency close |
| 6 | §8.5.9 + §4.5 | cancel-all → auto re-place exchange stop ≤ 2 s |
| 7 | §2.4 MVP mono-node | Обязательный этап; spot disabled; 3–5 futures |
| 8 | §1.3, §1 | Явная цепочка Binance→Bybit; momentum (не lag-arb) |
| 9 | §6.0 Spot policy | Futures-first; spot_min_tp_pct 0.8%; PF gate 1.3 |
| 10 | §9.1 | Replay follow-through ≥ 40% @ 150 ms delay |
| 11 | `[deployment]`, `[safe_mode]`, `[spot]` config | Новые секции config.toml |
| 12 | `BybitExitMetrics.volume_delta_100ms` | Warm path microstructure |

---

## 17. Изменения v1.6 → v1.7

| # | Добавлено | Описание |
|---|-----------|----------|
| 1 | §8.6 Analyst Service | Отдельный сервис: MA/EMA наблюдение, прогноз up/down/neutral |
| 2 | LLM forecast layer | JSON in/out; confidence; fallback rule-based |
| 3 | MA Binance + Bybit | EMA50/200/500; cross; divergence alert |
| 4 | Trade journal parquet | Export для Analyst и оператора |
| 5 | Go/No-Go checklist §8.6.7 | Обязательные критерии перед live |
| 6 | `analyst.toml` | Отдельный конфиг; port 8081 |
| 7 | Спринт 9 | Analyst после paper trading |
| 8 | Жёсткий запрет | Analyst не трейдер; не пишет в trading config |

---

## 18. Изменения v1.7 → v1.8

| # | Добавлено | Описание |
|---|-----------|----------|
| 1 | §10.6 Дорожная карта | **Фаза 1** (бот+Panel) / **Фаза 2** (БД+Analyst) |
| 2 | §8.6.4 Proposal & Apply | SuggestionQueue; Telegram [Apply][Reject]; Panel API |
| 3 | Типы suggestions | config, alloc, manual_entry, close, halt |
| 4 | §8.7 Order Book Warehouse | book-collector, TimescaleDB, паттерны стакана |
| 5 | Operator mobile | Push pending; `/pending`, `/apply {id}` |
| 6 | Фаза 2 gate | ≥14 дней БД, 100k snapshots, 50 сделок |
| 7 | book-collector | Старт в конце Фазы 1 (paper) |
| 8 | Auto-apply | Явно **запрещён** v1.8 |
