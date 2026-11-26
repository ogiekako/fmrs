import { OverlayTrigger, Popover } from "react-bootstrap";
import { BsInfoCircle } from "react-icons/bs";
import * as model from "../../model";

const futsuuInfoPopover = (
  <Popover id="futsuu-info-popover" style={{ maxWidth: "min(380px, 90vw)" }}>
    <Popover.Header as="h3" style={{ fontSize: "0.95rem" }}>
      普通詰将棋ソルバーの仕様と制限
    </Popover.Header>
    <Popover.Body style={{ fontSize: "0.85rem" }}>
      <ul className="mb-0 ps-3">
        <li>
          <strong>余詰検証・非限定・変化手順</strong>:
          余詰（複数解）や攻方の非限定の検出には未対応です。表示される手順は主手順 1 本のみで、受方の変化手順は分岐表示されません。
        </li>
        <li>
          <strong>最短手数</strong>:
          df-pn 探索の性質上、最短手数での詰みは保証されません（迂回手順や駒余りを含む手順が返る場合があります）。
        </li>
        <li>
          <strong>無駄合・駒余り</strong>:
          無駄合は可能な範囲で除外していますが、完全な除外は保証されません。詰み上がりに攻方の持駒が残る場合は「駒余り」と表示しますが、駒余りなしの別解探索は行いません。
        </li>
        <li>
          <strong>局面・手数の制約</strong>:
          最大 2000 手まで。壁駒（石）および初形で受方玉に王手がかかっている局面は非対応です。
        </li>
        <li>
          <strong>探索環境</strong>:
          サーバー接続時はサーバー側で解図し、接続できない場合はブラウザ内（WebAssembly）にフォールバックします。
        </li>
      </ul>
    </Popover.Body>
  </Popover>
);

export default function Info(props: { mode?: model.SolverMode }) {
  if (props.mode === "futsuu") {
    return (
      <div>
        <OverlayTrigger
          trigger="click"
          rootClose
          placement="bottom-end"
          overlay={futsuuInfoPopover}
        >
          <button
            type="button"
            className="btn btn-link p-0 border-0 lh-1 text-secondary"
            aria-label="普通詰将棋ソルバーの仕様と制限"
            title="仕様と制限"
          >
            <BsInfoCircle />
          </button>
        </OverlayTrigger>
      </div>
    );
  }
  return (
    <div>
      <a
        href="https://github.com/ogiekako/fmrs?tab=readme-ov-file#%E4%BD%BF%E3%81%84%E6%96%B9"
        target="_blank"
        rel="noreferrer"
      >
        <div>
          <BsInfoCircle className="text-secondary" />
        </div>
      </a>
    </div>
  );
}
