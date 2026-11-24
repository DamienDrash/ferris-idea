-- Oracle
SELECT q'[a;b/c]' AS t FROM dual;
BEGIN
  NULL;
END;
/
SELECT sysdate FROM dual FETCH FIRST 1 ROWS ONLY;
