package org.example.library.rest;

import jakarta.enterprise.context.RequestScoped;
import jakarta.inject.Inject;
import jakarta.inject.Named;
import jakarta.ws.rs.Consumes;
import jakarta.ws.rs.DELETE;
import jakarta.ws.rs.GET;
import jakarta.ws.rs.POST;
import jakarta.ws.rs.PUT;
import jakarta.ws.rs.Path;
import jakarta.ws.rs.PathParam;
import jakarta.ws.rs.Produces;
import jakarta.ws.rs.QueryParam;
import jakarta.ws.rs.core.MediaType;
import jakarta.ws.rs.core.Response;
import java.util.List;
import org.example.library.catalog.Catalog;
import org.example.library.model.Book;
import org.example.library.notify.Courier;
import org.example.library.notify.Express;
import org.example.library.notify.Sender;
import org.example.library.search.Indexer;
import org.example.library.search.SearchClient;

@Path("/books")
@RequestScoped
@Produces(MediaType.APPLICATION_JSON)
@Consumes(MediaType.APPLICATION_JSON)
public class BookResource {

    @Inject
    Catalog catalog;

    // Own qualifier: picks ExpressCourier among the two couriers.
    @Inject
    @Express
    Courier express;

    // No qualifier: the bean with @Default, StandardCourier.
    @Inject
    Courier standard;

    // Ambiguous: two senders, none preferred.
    @Inject
    Sender sender;

    // @Named qualifier.
    @Inject
    @Named("sms")
    Sender sms;

    // Missing: Indexer has no bean-defining annotation and no producer.
    @Inject
    Indexer indexer;

    // From a producer method.
    @Inject
    SearchClient search;

    @GET
    public List<Book> list(@QueryParam("q") String query) {
        return catalog.find(query);
    }

    @GET
    @Path("/{id}")
    @Produces(MediaType.APPLICATION_JSON)
    public Book get(@PathParam("id") long id) {
        return catalog.get(id);
    }

    @POST
    @Consumes("application/json")
    @Produces("application/json")
    public Response add(Book book) {
        catalog.add(book);
        return Response.status(Response.Status.CREATED).build();
    }

    @PUT
    @Path("{id}")
    public Response replace(@PathParam("id") long id, Book book) {
        catalog.add(book);
        return Response.ok().build();
    }

    @DELETE
    @Path("/{id}")
    public void remove(@PathParam("id") long id) {
    }
}
