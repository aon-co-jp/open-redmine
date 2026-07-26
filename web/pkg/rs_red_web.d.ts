/* tslint:disable */
/* eslint-disable */

/**
 * `web/index.html`の`onclick="delete_relation(...)"`から直接呼べるよう
 * グローバル公開する(`open_ticket`/`open_wiki_page`と同じパターン)。
 */
export function delete_relation(relation_id: bigint): void;

/**
 * `web/index.html`の`onclick="delete_time_entry(...)"`から直接呼べるよう
 * グローバル公開する。投稿者本人以外・非管理者が呼んだ場合はサーバー側が
 * `403`を返し、そのままエラー表示する(表示上の抑制は`load_time_entries`
 * 側で行うが、直接呼ばれた場合の最終防衛はサーバー側の権限チェック)。
 */
export function delete_time_entry(entry_id: bigint): void;

/**
 * チケット詳細を開く(詳細取得+コメント一覧取得)。JSの`onclick`から
 * `#[wasm_bindgen]`経由で直接呼べるようにグローバル公開する。
 */
export function open_ticket(ticket_id: bigint): void;

export function open_wiki_page(page_id: bigint): void;

export function select_project(project_id: bigint): void;

export function start(): void;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly select_project: (a: bigint) => void;
    readonly start: () => void;
    readonly delete_relation: (a: bigint) => void;
    readonly delete_time_entry: (a: bigint) => void;
    readonly open_ticket: (a: bigint) => void;
    readonly open_wiki_page: (a: bigint) => void;
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
