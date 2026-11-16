package example.notes;

import io.micronaut.context.annotation.ConfigurationProperties;

@ConfigurationProperties("notes")
public class NotesConfiguration {
    private String title;
    private int pageSize;

    public String getTitle() {
        return title;
    }

    public void setTitle(String title) {
        this.title = title;
    }

    public int getPageSize() {
        return pageSize;
    }

    public void setPageSize(int pageSize) {
        this.pageSize = pageSize;
    }
}
