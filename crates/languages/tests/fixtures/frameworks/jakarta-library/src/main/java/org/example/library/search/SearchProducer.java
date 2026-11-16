package org.example.library.search;

import jakarta.enterprise.context.ApplicationScoped;
import jakarta.enterprise.inject.Produces;
import jakarta.inject.Named;

@ApplicationScoped
public class SearchProducer {

    @Produces
    @ApplicationScoped
    public SearchClient searchClient() {
        return new SearchClient("http://localhost:9200");
    }

    // A producer field.
    @Produces
    @Named("libraryName")
    String libraryName = "City Library";
}
