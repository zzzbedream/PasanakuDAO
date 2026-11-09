# PRD Técnico: PasanakuDAO (TandaTrezu)

> **Estado de implementación (9 nov 2026).** Este documento es la especificación original con la
> que arrancó el proyecto. Durante la verificación técnica previa a implementar se encontraron
> cuatro puntos que no se corresponden con la realidad de las plataformas y se corrigieron. El
> código del repositorio sigue la versión corregida; la sección **"Desviaciones respecto al PRD"**
> al final de este archivo es la fuente de verdad cuando algo difiera.

## 1. Resumen Ejecutivo y Posicionamiento Estratégico

**Nombre del Proyecto:** PasanakuDAO (TandaTrezu)

**Concepto en una línea:** Infraestructura de ahorro y crédito rotativo (ROSCA: pasanakus, tandas,
pollas, natilleras) y subastas de liquidez comunitaria para América Latina, construida sobre
Sputnik DAO v2 y Trezu, con liquidación multicanal vía NEAR Intents y custodia verificable en
hardware Intel TDX (OutLayer).

**Tracks seleccionados (estrategia multi-track):**

- **Track primario:** 12 / `social` — Social Money (presupuestos compartidos, recompensas
  programables, propiedad colectiva y acceso basado en reputación sobre Sputnik DAO y Trezu).
- **Track secundario 1:** 08 / `realworld` — Real-World Money (rieles de stablecoins, pagos en
  lote y transacciones sin gas para comercios reales).
- **Track secundario 2:** 06 / `security` — AI Security & Defense (agente tesorero custodiado en
  Intel TDX con límites de gasto en hardware, claves de acceso acotadas y demo de ataque).

## 2. Arquitectura del repositorio

```
pasanaku-dao/
├── README.md                 # Guía para jueces: qué es, problema, pasos "haz X → ves Y"
├── HACKATHON.md              # Copia literal de las reglas oficiales
├── PRD.md                    # Este documento
├── LICENSE                   # MIT
├── verify.sh / verify.ps1    # Verificación integral (Rust + agente + web)
├── contracts/
│   ├── pasanaku-rosca/       # Contrato Rust: círculos, subasta, dividendos, reputación
│   ├── test-token/           # NEP-141 de prueba con mint público (sandbox / fallback)
│   ├── bulk-payment/         # Contrato de pagos en lote de Trezu (vendoreado, MIT)
│   ├── integration-tests/    # Tests de sandbox (near-workspaces, Linux/CI)
│   └── res/                  # sputnikdao2.wasm oficial + wasm compilados
├── agent/                    # Agente tesorero TS: política, firmantes, Telegram, API, attack-demo
├── web/                      # Next.js: rueda de turnos, subasta en vivo, panel del agente
├── scripts/                  # Despliegue testnet, seed, smoke, demo, verify
└── deployments/testnet.json  # Direcciones y hashes verificables
```

## 3. Modelo matemático de rondas, subasta y reputación

El contrato implementa un círculo de ahorro rotativo con **N** miembros y una cuota periódica fija
**C** denominada en USDC (NEP-141, 6 decimales).

- **Pozo bruto por ronda:** `P_bruto = N · C`, una vez que los N participantes depositan su cuota.
- **Subasta de turno por urgencia:** cualquier miembro que aún no haya cobrado puede ofertar un
  descuento `Δ` con `0 < Δ ≤ Δ_max`. La oferta ganadora `Δ*` es el mayor descuento **aprobado por
  la DAO** (Sputnik DAO v2, `VoteApprove`). Sin ofertas aprobadas, decide el orden de rotación.
- **Desembolso neto y dividendo social:**
  `P_neto = N · C − Δ*` para el adjudicatario y `d_i = Δ* / (N − 1)` para cada uno de los demás
  miembros (el residuo de la división entera vuelve al ganador; la suma siempre es el pozo).
- **Reputación on-chain:** `S(t+1) = S(t) + α · I_puntual + β · ln(1 + C) − γ · I_mora`, con
  α = 10, β = 5, γ = 25 y `ln` en aritmética entera (mili-nats, error < 1,5 %).
- **Niveles de acceso por reputación:** Semilla (`S ≥ 0`, tope 100 USDC por cuota), PyME
  (`S ≥ 100`, tope 500 USDC), Mayorista (`S ≥ 300`, tope 2 500 USDC).

## 4. Contrato inteligente `contracts/pasanaku-rosca`

Escrito en Rust con near-sdk 5.30. No reemplaza a Sputnik DAO v2 ni al contrato de pagos en lote
de Trezu: actúa como módulo de reglas de negocio gobernado por una instancia de Sputnik DAO v2
mediante propuestas `FunctionCall`.

| Método | Quién | Comportamiento |
|---|---|---|
| `create_circle` | un miembro (paga storage) | Valida miembros, reputación mínima y tope de nivel; crea el círculo en `Pending`. |
| `activate_circle` | DAO | Admite el círculo y abre la ronda 0. |
| `ft_on_transfer` | contrato del token | Acredita la cuota (`msg = {"circle_id","beneficiary"?}`); devuelve el exceso. |
| `submit_bid` | miembro sin cobrar | Oferta un descuento mayor al vigente y dentro del tope. |
| `approve_bid` | DAO | Aprueba una oferta exacta (bidder, descuento). |
| `close_round` | cualquiera | Si todos pagaron: calcula pagos, reputación y abre la siguiente ronda. Si venció el plazo: anula, reembolsa, penaliza y congela. |
| `execute_payouts` | cualquiera | Envía hasta 10 pagos por llamada con `ft_transfer` + callback. |
| `claim_payout` | destinatario o agente | Reintenta un pago fallido (p. ej. sin storage). |
| `freeze_circle`, `unfreeze_circle`, `remove_member`, `set_params` | DAO | Gobernanza del círculo. |
| Vistas | — | `get_circle`, `get_circles`, `get_round`, `get_rounds`, `get_round_status`, `get_reputation`, `get_member_tier`, `storage_cost_for_members`, `get_config`. |

Eventos NEP-297 bajo el estándar `pasanaku`: `circle_created`, `circle_activated`, `round_opened`,
`contribution_received`, `bid_submitted`, `bid_approved`, `round_closed`, `round_voided`,
`payout_sent`, `payout_confirmed`, `payout_failed`, `reputation_updated`, `circle_frozen`,
`circle_unfrozen`, `member_removed`, `params_updated`, `circle_completed`.

## 5. Agente tesorero (`agent/`)

Agente autónomo en TypeScript con **dos capas de política independientes**:

1. **Política local** (`agent/policy.json`): lista blanca de contratos y métodos, topes por
   transacción y por día, reglas sobre argumentos (`ft_transfer_call` solo hacia el contrato ROSCA;
   `add_proposal` solo con `FunctionCall` hacia métodos de gobernanza del ROSCA), transferencias
   nativas prohibidas, lista negra. Toda decisión queda en un registro de auditoría.
2. **Firmante acotado:** o bien **claves function-call** limitadas a un contrato y a métodos
   concretos (nunca una clave full-access), o bien una **billetera de custodia OutLayer** cuyas
   claves viven en un enclave Intel TDX y cuya política (topes, whitelist, aprobaciones multisig,
   congelamiento) la fija el dueño humano desde el panel de OutLayer.

El agente cierra rondas, ejecuta pagos, crea propuestas de aprobación de pujas en la DAO
(nunca vota), notifica por Telegram y expone una API REST con exportación CSV. La **demo de
ataque** (`pnpm attack-demo`) alimenta al agente con un mensaje de inyección de prompt que pide
transferir todo a una dirección maliciosa y demuestra el bloqueo en ambas capas.

## 6. Bot de Telegram

Comandos: `/start`, `/vincular`, `/tandas`, `/estado`, `/aportar`, `/subastar`, `/recibo`,
`/reputacion`, `/exportar`, `/congelar` (admin), `/auditoria`. El bot nunca firma por los
miembros: entrega enlaces profundos a la web para que cada uno firme con su propia billetera.

## 7. Frontend (`web/`)

Next.js con conexión de billetera (@hot-labs/near-connect): rueda interactiva de turnos, tarjeta
de subasta en vivo con el dividendo proyectado por miembro, diálogo de aporte (billetera NEAR o
cotización `dry` desde otra cadena vía 1Click), tabla de pagos con estados y enlaces al
explorador, insignia de reputación y panel del agente (modo de firma, custodia TEE verificable,
auditoría, exportación CSV).

## 8. Verificación automatizada

`pnpm verify` (o `./verify.sh` / `verify.ps1`): herramientas → tests unitarios Rust → compilación
wasm con checksums → (Linux) tests de sandbox → typecheck y tests del agente, scripts y web →
`attack-demo` → smoke test contra testnet si hay despliegue configurado. GitHub Actions ejecuta
lo mismo en cada push.

## 9. Cronograma (9 → 14 de noviembre de 2026)

| Día | Entregable |
|---|---|
| 9 nov | Contrato con tests unitarios y wasm; agente (política, firmantes, cliente OutLayer); cuenta testnet. |
| 10 nov | Despliegue testnet (token, ROSCA, bulk-payment, DAO), seed del círculo demo, CI verde con sandbox. |
| 11 nov | Telegram, quotes 1Click, web MVP. |
| 12 nov | README reproducible desde clon limpio, demo e2e, congelación de features. |
| 13 nov | Vídeo, post en X, limpieza, repo público. |
| 14 nov | Verificación final y envío del formulario (con confirmación humana). |

---

## Desviaciones respecto al PRD original

1. **OutLayer se integra con su API real.** El borrador original citaba
   `POST /v1/wallet/simulate`, `POST /v1/intents/swap-and-call`, una "tarifa de 1 pip" y una "cita
   de atestación Intel TDX por cada swap". Esos endpoints y garantías **no existen** en la
   documentación oficial (`skills.outlayer.ai`). La integración usa `POST /register`,
   `/wallet/v1/call`, `/wallet/v1/transfer`, `/wallet/v1/storage-deposit`,
   `/wallet/v1/requests/{id}`, `/wallet/v1/audit`, `/wallet/v1/pending_approvals` y, solo en
   mainnet, `/wallet/v1/intents/*` y `/wallet/v1/payment-check/*`. La verificación TEE se apoya en
   el contrato de registro de workers (`worker.outlayer.testnet` / `worker.outlayer.near`) y en el
   registro de auditoría, no en un quote por llamada. Las comisiones de swap son las de 1Click.
2. **No se forkea el monorepo de Trezu.** `nt-be` requiere Postgres y una caché de consultas
   sqlx; no es reproducible por un juez automatizado en el plazo. "Construido sobre Sputnik DAO y
   Trezu" se cumple con una DAO Sputnik v2 real para toda la gobernanza, el contrato bulk-payment
   de Trezu vendoreado sin cambios y desplegado en testnet, y la misma semántica de estados y CSV.
3. **Sin passkeys ni patrocinio de gas propio.** Dependen de la infraestructura hospedada de
   Trezu. Los miembros con billetera firman con @hot-labs/near-connect; los miembros que solo usan
   Telegram no tocan gas porque el agente tesorero ejecuta las transacciones por ellos.
4. **Solo testnet y sin track 03.** NEAR Intents no tiene testnet: las cotizaciones 1Click se
   muestran reales en modo `dry`, etiquetadas como "ejecución solo en mainnet". El track
   03 / `privacy` (NEAR AI Cloud) se descarta para concentrar el alcance en tres tracks.
