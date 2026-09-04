use mailkit::message::EmailMessage;
use proptest::prelude::*;

proptest! {
    #[test]
    fn email_message_builder_valid(
        from in "[a-z]{1,20}@[a-z]{1,20}\\.[a-z]{2,5}",
        to in "[a-z]{1,20}@[a-z]{1,20}\\.[a-z]{2,5}",
        subject in "[a-zA-Z ]{0,100}",
    ) {
        let msg = EmailMessage::builder()
            .from(from.clone())
            .to(to.clone())
            .subject(subject.clone())
            .build()
            .unwrap();

        prop_assert_eq!(&msg.from, &from);
        prop_assert_eq!(&msg.to, &[to]);
        prop_assert_eq!(&msg.subject, &subject);
    }

    #[test]
    fn email_message_builder_multiple_recipients(
        from in "[a-z]{1,20}@[a-z]{1,20}\\.[a-z]{2,5}",
        recipients in prop::collection::vec("[a-z]{1,20}@[a-z]{1,20}\\.[a-z]{2,5}", 1..10),
    ) {
        let mut builder = EmailMessage::builder().from(from).subject("test");
        for r in &recipients {
            builder = builder.to(r.clone());
        }
        let msg = builder.build().unwrap();
        prop_assert_eq!(msg.to.len(), recipients.len());
    }

    #[test]
    fn email_message_builder_no_from_fails(
        to in "[a-z]{1,20}@[a-z]{1,20}\\.[a-z]{2,5}",
    ) {
        let result = EmailMessage::builder()
            .to(to)
            .subject("no sender")
            .build();
        prop_assert!(result.is_err());
    }

    #[test]
    fn email_message_builder_no_recipient_fails(
        from in "[a-z]{1,20}@[a-z]{1,20}\\.[a-z]{2,5}",
    ) {
        let result = EmailMessage::builder()
            .from(from)
            .subject("no recipient")
            .build();
        prop_assert!(result.is_err());
    }

    #[test]
    fn email_message_subject_defaults_to_empty(
        from in "[a-z]{1,20}@[a-z]{1,20}\\.[a-z]{2,5}",
        to in "[a-z]{1,20}@[a-z]{1,20}\\.[a-z]{2,5}",
    ) {
        let msg = EmailMessage::builder()
            .from(from)
            .to(to)
            .build()
            .unwrap();
        prop_assert_eq!(&msg.subject, "");
    }
}
