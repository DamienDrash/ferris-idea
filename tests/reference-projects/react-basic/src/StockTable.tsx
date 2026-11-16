export interface StockValue {
  warehouseId: number;
  valueCents: number;
}

export function formatCents(cents: number): string {
  return (cents / 100).toFixed(2);
}

export function StockTable({ rows }: { rows: StockValue[] }) {
  return (
    <table>
      <tbody>
        {rows.map((row) => (
          <tr key={row.warehouseId}>
            <td>{row.warehouseId}</td>
            <td>{formatCents(row.valueCents)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
