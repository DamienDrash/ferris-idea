package lab.inventory;

import java.util.List;
import org.springframework.jdbc.core.simple.JdbcClient;
import org.springframework.stereotype.Repository;

@Repository
public class StockRepository {
    public record StockValue(int warehouseId, long valueCents) {}

    private final JdbcClient jdbc;

    public StockRepository(JdbcClient jdbc) {
        this.jdbc = jdbc;
    }

    public List<StockValue> values() {
        return jdbc.sql("select warehouse_id, value_cents from stock_value order by warehouse_id")
                .query(StockValue.class)
                .list();
    }
}
