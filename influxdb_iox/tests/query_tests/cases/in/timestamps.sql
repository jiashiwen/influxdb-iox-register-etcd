-- Timestamp printing / output testss
-- IOX_SETUP: OneMeasurementRealisticTimes

-- Expect the timestamp output to be formatted correctly (with `Z`)
SELECT * from cpu;
-- explicit offset format
SELECT * FROM cpu WHERE time  > to_timestamp('2021-07-20 19:28:50+00:00');
-- Use RCF3339 format
SELECT * FROM cpu WHERE time  > to_timestamp('2021-07-20T19:28:50Z');
--use cast workaround
SELECT * FROM cpu WHERE
  CAST(time AS BIGINT) > CAST(to_timestamp('2021-07-20T19:28:50Z') AS BIGINT);
-- work aroound for case time > 10 order by region, time, user;
 SELECT * FROM cpu where cast(time as bigint) > 10  order by region, time, "user";
 -- this query does not work: SELECT * FROM cpu where time > 10
