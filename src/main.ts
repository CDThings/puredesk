// 入口路由：主面板 (#/) 与收纳盒 (#/box/<id>) 共用一个 bundle
import "./style.css";
import { initBox } from "./ui/box";
import { initMain } from "./ui/main";

const m = location.hash.match(/^#\/box\/(.+)$/);
if (m) {
  initBox(decodeURIComponent(m[1]));
} else {
  initMain();
}
