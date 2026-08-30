use sqlx::{Postgres, Transaction};
use uuid::Uuid;

pub(crate) async fn get_or_create_direct_room(
    transaction: &mut Transaction<'_, Postgres>,
    first_participant: &str,
    second_participant: &str,
) -> Result<(String, bool), sqlx::Error> {
    if first_participant == second_participant {
        return Err(sqlx::Error::Protocol(
            "INVALID_ARGUMENT: a direct room needs two participants".to_string(),
        ));
    }
    let (left, right) = if first_participant < second_participant {
        (first_participant, second_participant)
    } else {
        (second_participant, first_participant)
    };
    let direct_key = format!("{left}|{right}");
    let proposed_room_id = format!("room_{}", Uuid::new_v4().simple());
    let inserted: Option<String> = sqlx::query_scalar(
        "INSERT INTO collab_rooms (id, kind, direct_key)
         VALUES ($1, 'direct', $2)
         ON CONFLICT (direct_key) WHERE direct_key IS NOT NULL DO NOTHING
         RETURNING id",
    )
    .bind(&proposed_room_id)
    .bind(&direct_key)
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some(room_id) = inserted {
        sqlx::query(
            "INSERT INTO collab_room_members (room_id, participant_id, last_read_seq)
             VALUES ($1, $2, 0), ($1, $3, 0)",
        )
        .bind(&room_id)
        .bind(left)
        .bind(right)
        .execute(&mut **transaction)
        .await?;
        Ok((room_id, true))
    } else {
        let room_id =
            sqlx::query_scalar("SELECT id FROM collab_rooms WHERE direct_key = $1 FOR UPDATE")
                .bind(direct_key)
                .fetch_one(&mut **transaction)
                .await?;
        Ok((room_id, false))
    }
}
