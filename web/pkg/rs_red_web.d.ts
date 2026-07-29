/* tslint:disable */
/* eslint-disable */

/**
 * `web/index.html`の`onclick="delete_relation(...)"`から直接呼べるよう
 * グローバル公開する(`open_ticket`/`open_wiki_page`と同じパターン)。
 * `u32`で受ける理由は`select_project`と同じ(2026-07-27追記、
 * `TypeError: Cannot convert 0 to a BigInt`の回避)。
 */
export function delete_relation(relation_id: number): void;

/**
 * `web/index.html`の`onclick="delete_time_entry(...)"`から直接呼べるよう
 * グローバル公開する。投稿者本人以外・非管理者が呼んだ場合はサーバー側が
 * `403`を返し、そのままエラー表示する(表示上の抑制は`load_time_entries`
 * 側で行うが、直接呼ばれた場合の最終防衛はサーバー側の権限チェック)。
 * `u32`で受ける理由は`select_project`と同じ(2026-07-27追記、
 * `TypeError: Cannot convert 0 to a BigInt`の回避)。
 */
export function delete_time_entry(entry_id: number): void;

/**
 * チケット詳細を開く(詳細取得+コメント一覧取得)。JSの`onclick`から
 * `#[wasm_bindgen]`経由で直接呼べるようにグローバル公開する。
 * `u32`で受ける理由は`select_project`と同じ(2026-07-27追記、
 * `TypeError: Cannot convert 0 to a BigInt`の回避)。
 */
export function open_ticket(ticket_id: number): void;

/**
 * `u32`で受ける理由は`select_project`と同じ(2026-07-27追記、
 * `TypeError: Cannot convert 0 to a BigInt`の回避)。
 */
export function open_wiki_page(page_id: number): void;

/**
 * **2026-07-27追記(実クリックE2Eで発見した実バグの修正)**: 引数は
 * `u32`で受ける。理由——`wasm-bindgen`は`u64`をJS側`BigInt`へ写像するが、
 * `web/index.html`側の`onclick="select_project({id})"`は通常のJS数値
 * リテラル(例: `select_project(0)`、BigIntリテラルの`0n`ではない)を
 * 埋め込んでいたため、実ブラウザでボタンをクリックすると
 * `TypeError: Cannot convert 0 to a BigInt`で握りつぶされ、
 * プロジェクト選択自体が一切機能していなかった(`cargo test`はJS↔WASMの
 * 呼び出し境界を経由しないため、この種の不具合は検出できない——実際に
 * ブラウザで実クリックして初めて発覚した)。`u32`ならJS側は通常の
 * `Number`として渡せるため、この変換エラーが起きない。
 */
export function select_project(project_id: number): void;

export function start(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly select_project: (a: number) => void;
    readonly start: () => void;
    readonly open_ticket: (a: number) => void;
    readonly open_wiki_page: (a: number) => void;
    readonly delete_relation: (a: number) => void;
    readonly delete_time_entry: (a: number) => void;
    readonly wasm_bindgen__convert__closures_____invoke__h0420fbd9399d5376: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen__convert__closures_____invoke__hb9cd8f1b0a40051a: (a: number, b: number) => void;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
