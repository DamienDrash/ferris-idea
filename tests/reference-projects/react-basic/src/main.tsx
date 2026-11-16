import { createRoot } from "react-dom/client";
import { StockTable } from "./StockTable";

createRoot(document.getElementById("root")!).render(
  <StockTable rows={[{ warehouseId: 1, valueCents: 2500 }]} />,
);
