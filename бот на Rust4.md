# 📘 ТЕХНИЧЕСКОЕ ЗАДАНИЕ (ТЗ)
## Low-Latency Алготрейдинговая Система: Binance Futures → Bybit Spot/Perpetual
**Версия:** 1.3  
**Дата:** 08.07.2026  
**Язык разработки:** Rust 1.78+  
**Среда исполнения:** Linux (Ubuntu 22.04/24.04 LTS), `x86_64`/`aarch64`  
**Архитектура:** Распределённая двухузловая (`Observer` → `Executor`)  

> **Changelog v1.3:** формализовано принятие решения о входе (Observer only), исправлена машина состояний SL/TP, уточнена стратегия (lead-lag/momentum), политика Spot Short, единицы измерения, Risk Engine hot/warm path, freshness 150 ms, exit-EMA по Bybit, fail-safe Spot, политика потерь UDP, `packet_version = 2`.  
> **Changelog v1.2:** добавлен динамический Take-Profit (§5.4) с трейлингом, частичной фиксацией и интеграцией со стоп-лоссом.  
> **Changelog v1.1:** исправлена синхронизация времени между узлами, устранены противоречия в структурах данных и конфиге, интегрированы модули §11, уточнена логика Long/Short и Spot/Futures.

---

## 1. Общие положения и цели системы

| Параметр | Спецификация |
|----------|--------------|
| **Назначение** | Автоматизированная **кросс-биржевая импульсная торговля (lead-lag / momentum)** с минимальной задержкой. Сбор рыночных метрик с **Binance Futures** (единственный источник сигналов), передача по внутренней сети AWS, финальная валидация рисками, исполнение и сопровождение позиций на **Bybit Spot** и **Bybit USDT Perpetual** (Long; Short — см. §4.3). **Не является классическим арбитражем:** basis filter (§4.2) сознательно отсекает расхождения цен > 0.05%; система ставит на коррелированное движение Bybit вслед за импульсом Binance Futures. |
| **Принцип разделения** | `Observer` (Токио) = сбор данных, фильтрация шума, расчёт метрик, **полная оценка условий входа §7**, генерация `MarketStatePacket` с `entry_valid` + `direction_bias`.<br>`Executor` (Сингапур) = freshness/dedup, **Risk Engine (только фильтры исполнения)**, маршрутизация Spot/Futures, исполнение, ведение позиции, **локальный EMA для exit-триггеров**, управление капиталом.<br>**Дублирование entry-логики (Z, D_exp, D_min) на Executor запрещено.** |
| **Технологический стек** | Rust, `tokio`, `simd-json`, `zenoh`, `postcard`, `crossbeam`, `tracing`, `prometheus`, `rustls`, `teloxide` (только на Executor / отдельном сервисе алертов). |
| **Инфраструктура** | AWS: `ap-northeast-1` (Токио), `ap-southeast-1` (Сингапур). Связь строго по внутренним IP через AWS Backbone (VPC Peering / Transit Gateway). |
| **Допустимые инструменты** | Статический Whitelist: 20–35 пар. Динамическая подписка/отписка в процессе работы запрещена. |
| **Ключевые ограничения** | One-way latency Токио→Сингапур P95 ≤ 80 мс, P99 ≤ 110 мс. Freshness drop > 150 мс. Hot path Risk Engine ≤ 10 мкс (§4.2). Проскальзывание входа ≤ 0.05%. Максимальный дневной DD: Spot ≤ 2%, Futures ≤ 1.5%. |

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
3. **Spot и Futures разделены на уровне коннекторов** в Executor; маршрутизация через `symbols.toml`.
4. **Long и Short** — симметричная логика входа/выхода с инверсией условий (§7).

### 1.2. Принятие решения о входе (единственный источник — Observer)

```
[Tick Binance] → Noise Filter → Metrics (Z, Vel, EMA, ATR, regime)
       ↓
  Entry Engine (§7): D_exp, D_min, Z_threshold, regime matrix
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

### 1.3. Единицы измерения (обязательны для реализации)

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
- **Heartbeat:** Отдельный топик `system/heartbeat/tokyo`, пакет с `ts_ns` каждые 100 мс. Timeout: 500 мс → триггер `Safe-Mode`.
- **Таймстампы:** `ts_ns` = **UTC wall-clock** (`CLOCK_REALTIME`, наносекунды с эпохи). Синхронизация: `chrony` со stratum ≤ 2 на обоих узлах. **Запрещено** использовать `CLOCK_MONOTONIC` в межузловых пакетах.
- **Политика потерь UDP:**
  - **Dedup:** `seq_num <= last_seq_num[symbol_id]` → drop.
  - **Gap detection:** `seq_num > last + 1` → `seq_gap_count++`, лог `WARN`.
  - **Gap storm:** если `seq_gap_count > 10` за 1 с по символу → `pause_entries[symbol_id]` на 5 с, алерт.
  - Потерянные пакеты **не интерполируются**; следующий валидный пакет принимается как есть.
- **Safe-Mode RTT:** скользящий P95 one-way latency за 10 с > **150 мс** → Safe-Mode (§5.3). Измерение — тот же `utc_now_ns() − packet.ts_ns`, не round-trip heartbeat.

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
- **Микроструктурные метрики:** `bid_ask_imbalance`, `volume_delta_100ms` — из `@bookTicker` + `@aggTrade` (справочно; microstructure filter на Executor).
- **Z_threshold (приоритет):**
  - `use_dynamic_thresholds = false` → `z_score_entry` из config (по умолчанию 2.5).
  - `use_dynamic_thresholds = true` → `Z_threshold = clamp(percentile(|Z|, 5000, 0.95) × 1.1, 1.8, 3.2)`; **`z_score_entry` игнорируется**.
- **Entry Engine (после метрик):** вычисляет `D_min`, `D_exp`, проверяет условия §7; выставляет `entry_valid`, `direction_bias`, записывает `d_exp`, `d_min`, `sigma`, `z_threshold_used` в пакет.
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
- `@bookTicker` Bybit WS → spread, depth, `bybit_mid` (каждый тик).
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
| `MICRO_OK` | Long: `volume_delta_100ms ≥ 0`; Short: `volume_delta_100ms ≤ 0` |
| `SPOT_SIDE_OK` | Short на Spot только при `spot_margin_enabled` (§4.3) |
| `PAUSE_OK` | Нет `pause_entries` после gap storm (§2.2) |

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
- **Crash / SIGKILL:** exchange stop остаётся на бирже — единственная защита Spot при падении Executor.
- **Watchdog:** `systemd` `WatchdogSec=30`, `Restart=always`. При недоступности > 30 s + открытые позиции → Alertmanager `CRITICAL` (оператор вручную через `/flush` или биржу).

---

## 5. Управление позициями и логика выхода

### 5.0. Единая машина состояний SL / TP

Все пороги — `pnl_pct` от entry (§1.3). Обновление — каждый `MarketStatePacket` + `Bybit @bookTicker` mid для PnL и SL/TP trigger.

```
[OPEN]
  SL = Entry ∓ ATR×1.8          (initial)
  TP_fixed = Entry × (1 ± 0.5%)  (TP-0 target, виртуальный)
       │
  PnL ≥ 0.15% ──→ SL-1: Entry ∓ ATR×0.5
       │
  PnL ≥ 0.30% ──→ SL-BE: Stop = Entry (безубыток)
       │           TP phase → TrailArm (начало трейлинга остатка)
       │
  Price crosses TP_fixed ──→ TP-0: закрыть 50% (partial_close_pct)
       │           SL остаётся Entry на остатке
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
| ≥ 0.30% | Entry (BE) | TrailArm | Трейлинг TP активен, partial ещё нет |
| Cross TP_fixed (+0.5%) | Entry (BE) | DynamicTrail | Partial 50%; SL/TP на остатке |
| Extended (opt.) | BE или trail | Extended | Futures + Trend only |

**Long:** SL только повышается; TP trail — `max(TP_old, Price − ATR×K_tp)`.  
**Short:** SL только понижается; TP trail — `min(TP_old, Price + ATR×K_tp)`.

### 5.1. Виртуальный стоп и синхронизация с биржей
- Стоп по умолчанию **виртуальный** (в памяти Executor).
- **Futures:** реальный `Stop-Market` на Bybit обязателен; amend каждые 5 с или при `ΔATR > 15%`. Ликвидационная цена локально; `Distance_to_Liq < 0.4%` → закрытие 50%.
- **Spot:** виртуальный SL + optional exchange Stop-Limit (§4.5). При `heartbeat_timeout > 500 ms` → немедленный market close всех Spot-позиций.

### 5.2. Триггеры выхода

| Триггер | Условие | Источник данных | Действие |
|---------|---------|-----------------|----------|
| Stop-Loss | `bybit_mid` пересекает `current_stop` | Bybit bookTicker | Market (Limit fallback §4.4) |
| Take-Profit | `bybit_mid` пересекает `current_tp` | Bybit bookTicker | Partial / full (§5.0) |
| Exhaustion | `\|Vel_binance\| < 0.00005` ∧ `\|Z_binance\| < 0.5` | Пакет Observer | Market close |
| EMA Cross | `bybit_ema_50` пересекает `bybit_ema_200` против позиции | **Executor local EMA on Bybit mid** | Market close |
| Spread Expansion | Bybit spread > 0.01% ∨ depth_10 < $50k | Bybit bookTicker | Limit ±0.02% |
| Safe-Mode | Heartbeat loss / P95 latency > 150 ms | §2.2 | Block entries, close all |

**Приоритет:** Safe-Mode → Stop-Loss → Take-Profit → EMA Cross → Exhaustion → Spread Expansion.

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
| PnL ≥ 0.15% | SL-1 (Entry ∓ ATR×0.5) | Initial |
| PnL ≥ 0.30% | BE (Entry) | TrailArm |
| TP-0 partial 50% | BE на остатке | DynamicTrail |
| SL до TP-0 | Full close | Cancelled |
| `take_profit.enabled = false` | §5.0 SL only | Disabled |

---

## 6. Управление капиталом и мультиинструментальность

### 6.1. Слоты и размер позиции
- Депозит делится на 5 слотов (по 20%).
- **Формула размера (Futures):** `$Qty = \frac{Slot \times Risk\%}{ATR \times Multiplier}$`
- **Формула размера (Spot):** та же формула; `Lev = 1`.
- Келли: **только оффлайн** для калибровки `Risk%` и `Multiplier`.
- Динамическая адаптация: ликвидность недостаточна → объём уменьшается до `slippage ≤ 0.05%`.

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
| **Мин. движение (Spot)** | `$D_{min} = Fee_{in} + Fee_{out} + Slippage + Buffer$` | `Fee=0.001`, `Slippage=0.0003`, `Buffer=0.0003` |
| **Мин. движение (Futures)** | `$D_{min} = (Fee_{in} + Fee_{out} + Slippage) \times Lev + Buffer$` | `Fee=0.00055`, `Lev=10`, `Buffer=0.0003` |
| **Ожидаемое движение** | `$D_{exp} = \alpha \cdot \|Z\| \cdot \sigma + \beta \cdot \|Vel\| \cdot \Delta t$` | `α=0.4`, `β=0.6`, `Δt=0.3` с |
| **Вход Long** | `D_exp ≥ D_min` ∧ `\|Z\| ≥ Z_threshold` ∧ `Vel > velocity_min` ∧ `EMA_50 > EMA_200` | см. regime matrix |
| **Вход Short** | `D_exp ≥ D_min` ∧ `\|Z\| ≥ Z_threshold` ∧ `Vel < −velocity_min` ∧ `EMA_50 < EMA_200` | см. regime matrix |
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
| Paper Trading | Bybit Testnet + Binance Futures Testnet | ≥ 100 сделок Futures + Spot Long, DD < 1% |
| Live Staged | 1% депозита | 7 дней без критических ошибок |

### 9.2. CI/CD и развертывание
- Pipeline: GitHub Actions → fmt → clippy → test → release → S3 → SSH deploy.
- Сервисы: `systemd` (`observer.service`, `executor.service`, `telegram-alerts.service`).
- Конфиг: `/etc/bot/config.toml`, `/etc/bot/symbols.toml`.
- Rollback: документирован в Runbook.

---

## 10. Приложения

### 10.1. `config.toml` (единый, без дубликатов секций)
```toml
[capital]
initial_deposit_usdt = 500.0
risk_per_trade_pct = 0.01
slot_count = 5

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
sl_breakeven_pct = 0.003             # PnL ≥ 0.3%: SL → Entry
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

[[symbol]]
id = 2
binance = "ETHUSDT"
bybit = "ETHUSDT"
instrument = "spot"
spot_margin_enabled = false   # Long only; Short → RISK_SKIP

[[symbol]]
id = 3
binance = "SOLUSDT"
bybit = "SOLUSDT"
instrument = "futures"
leverage = 5

# Пример Spot Margin (опционально):
# [[symbol]]
# id = 4
# binance = "XRPUSDT"
# bybit = "XRPUSDT"
# instrument = "spot"
# spot_margin_enabled = true    # Short через Bybit Spot Margin API
```

### 10.3. Telegram-бот (Executor)
- `/status` → Позиции, PnL, Latency, Mode, RiskFlags
- `/pause` → Блокировка новых входов
- `/resume` → Снятие блокировки
- `/flush` → Экстренное закрытие всех позиций + полный стоп

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
}
```

### 10.5. Порядок внедрения (спринты)
| Спринт | Модуль | Зависимости |
|--------|--------|-------------|
| 1 | `.bin` лог + Tick-Replay | postcard, tokio, memmap2 |
| 2 | Observer: WS + парсинг + RingBuffer + Entry Engine §7 | simd-json, zenoh |
| 3 | Executor: Risk hot/warm + Bybit connectors | Bybit V5 WS |
| 4 | SL/TP state machine §5.0 + partial close + Spot fail-safe | PositionState |
| 5 | Dynamic Z_threshold + Regime matrix | §3.3, config |
| 6 | Funding/Basis warm cache + gap storm policy | Bybit REST |
| 7 | Bybit exit EMA + Paper → Live staged | Runbook, Telegram |

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
