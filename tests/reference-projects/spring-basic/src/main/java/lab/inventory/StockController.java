package lab.inventory;

import java.util.List;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;

@RestController
public class StockController {
    private final StockRepository repository;

    public StockController(StockRepository repository) {
        this.repository = repository;
    }

    @GetMapping("/stock/value")
    public List<StockRepository.StockValue> stockValue() {
        return repository.values();
    }
}
