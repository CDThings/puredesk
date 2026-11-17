// 入口路由：主面板 (#/)、收纳盒 (#/box/<id>) 与收纳岛 (#/island) 共用一个 bundle
import "./style.css";
import { initBox } from "./ui/box";
import { initIsland } from "./ui/island";
import { initMain } from "./ui/main";

const m = location.hash.match(/^#\/box\/(.+)$/);
if (m) {
  initBox(decodeURIComponent(m[1]));
} else if (location.hash === "#/island") {
  initIsland();
} else {
  initMain();
}
