package org.example.library.catalog;

import jakarta.enterprise.context.ApplicationScoped;
import jakarta.inject.Inject;
import java.util.List;
import org.example.library.model.Book;
import org.example.library.search.SearchClient;

@ApplicationScoped
public class Catalog {

    private final SearchClient search;

    @Inject
    public Catalog(SearchClient search) {
        this.search = search;
    }

    public List<Book> find(String query) {
        return List.of();
    }

    public Book get(long id) {
        return new Book();
    }

    public void add(Book book) {
    }
}
