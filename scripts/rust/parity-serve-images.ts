/** Serve-stage image cases: TS base64 answers against Rust raw bytes. */
import { report } from "./parity-lib";
import { api, type Pair, rpc } from "./parity-serve-lib";

interface Tally {
  cases: number;
}

const seg = encodeURIComponent;

/** Image hit, miss and tool-result item on the generated transcript. */
export const imageCases = async (pair: Pair, c: Tally, id: string) => {
  const refs: [number, number, number | undefined][] = [
    [0, 1, undefined],
    [0, 0, undefined],
    [2, 0, 0],
    [2, 0, 5],
    [1, 0, undefined],
  ];
  for (const [line, block, item] of refs) {
    c.cases += 1;
    await imageCase(pair, id, line, block, item);
  }
};

const imageCase = async (
  pair: Pair,
  id: string,
  line: number,
  block: number,
  item: number | undefined
) => {
  const label = `sessions.image ${line}/${block}/${item ?? "-"}`;
  const exit = await rpc(pair.ts.port, "sessions.image", {
    id,
    line,
    block,
    ...(item === undefined ? {} : { item }),
  });
  const tail = item === undefined ? "" : `/${item}`;
  const r = await api(
    pair.rust.port,
    `/api/sessions/${seg(id)}/images/${line}/${block}${tail}`
  );
  if (exit.kind !== "ok") {
    report(`serve ${label}: ts ${exit.kind}`);
    return;
  }
  const value = exit.value as { data: string; mediaType: string } | null;
  if (value === null) {
    const tag = (r.json as { error?: { _tag?: string } } | undefined)?.error
      ?._tag;
    if (r.status !== 404 || tag !== "ImageNotFoundError") {
      report(`serve ${label}: ts null, rust ${r.status} ${tag}`);
    }
    return;
  }
  const data = Buffer.from(r.body).toString("base64");
  if (r.status !== 200 || data !== value.data || r.type !== value.mediaType) {
    report(
      `serve ${label}: ts ${value.mediaType} vs rust ${r.status} ${r.type}`
    );
  }
};
