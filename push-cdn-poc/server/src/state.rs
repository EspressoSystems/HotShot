use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
};

use common::topic::Topic;

pub struct ConnectionMap<Key, Connection> {
    topic_to_keys: HashMap<Topic, HashSet<Key>>,
    key_to_topics: HashMap<Key, HashSet<Topic>>,

    key_to_connection: HashMap<Key, Connection>,
}

impl<Key, Connection> Default for ConnectionMap<Key, Connection> {
    fn default() -> Self {
        Self {
            topic_to_keys: HashMap::new(),
            key_to_topics: HashMap::new(),
            key_to_connection: HashMap::new(),
        }
    }
}

impl<Key: Eq + Hash + Copy, Connection: Eq + Hash + Clone> ConnectionMap<Key, Connection> {
    pub fn get_connections_for_broadcast(
        &self,
        _sender: Key,
        topics: Vec<Topic>,
    ) -> HashSet<Connection> {
        let mut interested_connections: HashSet<Connection> = HashSet::new();

        // get all interested parties
        for topic in topics {
            if let Some(keys) = self.topic_to_keys.get(&topic) {
                for key in keys {
                    if let Some(connection) = self.key_to_connection.get(key) {
                        interested_connections.insert(connection.clone());
                    }
                }
            }
        }

        // TODO: SHORT CIRCUIT SEND OURSELVES
        // if let Some(connection) = self.key_to_connection.get(&sender) {
        //     connections.remove(connection);
        // }
        interested_connections
    }

    pub fn get_connection_for_direct(&self, key: Key) -> Option<Connection> {
        self.key_to_connection.get(&key).cloned()
    }

    pub fn subscribe_key_to(&mut self, key: Key, topics: Vec<Topic>) {
        //topic -> [key]
        for topic in topics.clone() {
            self.topic_to_keys.entry(topic).or_default().insert(key);
        }
        //key -> [topic]
        self.key_to_topics.entry(key).or_default().extend(topics);
    }

    pub fn unsubscribe_key_from(&mut self, key: Key, topics: Vec<Topic>) {
        //topic -> [key]
        for topic in topics.clone() {
            // remove key from topic, and remove topic if empty
            if let Some(keys) = self.topic_to_keys.get_mut(&topic) {
                keys.remove(&key);
            }
        }

        //key -> [topic]
        if let Some(key_topics) = self.key_to_topics.get_mut(&key) {
            for topic in topics {
                key_topics.remove(&topic);
            }
        }
    }

    pub fn add_connection(
        &mut self,
        key: Key,
        connection: Connection,
        initial_topics: HashSet<Topic>,
    ) -> Option<Connection> {
        // topics <-> key
        for topic in initial_topics.clone() {
            self.topic_to_keys.entry(topic).or_default().insert(key);
        }

        self.key_to_topics.insert(key, initial_topics);

        // connection <-> key
        self.key_to_connection.insert(key, connection)
    }

    pub fn remove_connection(&mut self, key: Key) -> Option<Connection> {
        // topics <-> key
        if let Some(topics) = self.key_to_topics.remove(&key) {
            for topic in topics {
                if let Some(keys) = self.topic_to_keys.get_mut(&topic) {
                    keys.remove(&key);
                }
            }
        }

        // connection <-> key
        self.key_to_connection.remove(&key)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_initial() {
        // create new connection map and topics
        let mut connection_map = ConnectionMap::default();
        let initial_topics = HashSet::from([Topic::Global, Topic::Global]);

        // add a key to the connection map for the topics
        let key = 0;
        let connection = 0;
        connection_map.add_connection(key, connection, initial_topics);

        // see if broadcast includes connection
        let topics = vec![Topic::Global];
        let connections = connection_map.get_connections_for_broadcast(key, topics.clone());
        assert_eq!(connections.len(), 1);
        assert!(connections.contains(&connection));

        // see if direct includes connection
        let possible_connection = connection_map.get_connection_for_direct(key);
        assert_eq!(possible_connection, Some(0));

        // remove connection, check broadcast
        connection_map.remove_connection(key);
        let connections = connection_map.get_connections_for_broadcast(key, topics);
        assert_eq!(connections.len(), 0);
        assert!(!connections.contains(&connection));

        // check direct
        let connection = connection_map.get_connection_for_direct(key);
        assert_eq!(connection, None)
    }

    #[test]
    fn test_duplicate_keys() {
        // create new connection map and topics
        let mut connection_map = ConnectionMap::default();
        let initial_topics = HashSet::from([Topic::Global, Topic::Global]);

        // add a key to the connection map for the topics
        let key = 0;
        let connection = 0;
        connection_map.add_connection(key, connection, initial_topics.clone());

        // see if broadcast includes connection
        let topics = vec![Topic::Global];
        let connections = connection_map.get_connections_for_broadcast(key, topics.clone());
        assert_eq!(connections.len(), 1);
        assert!(connections.contains(&connection));

        // see if direct includes connection
        let possible_connection = connection_map.get_connection_for_direct(key);
        assert_eq!(possible_connection, Some(0));

        // add again
        connection_map.add_connection(key, connection, initial_topics);

        // see if broadcast includes connection
        let topics = vec![Topic::Global];
        let connections = connection_map.get_connections_for_broadcast(key, topics.clone());
        assert_eq!(connections.len(), 1);
        assert!(connections.contains(&connection));

        // see if direct includes connection
        let possible_connection = connection_map.get_connection_for_direct(key);
        assert_eq!(possible_connection, Some(0));
    }

    #[test]
    fn test_subscribe_unsubscribe() {
        // create new connection map and empty topic set
        let mut connection_map = ConnectionMap::default();
        let initial_topics = HashSet::from([]);

        // add a key to the connection map for no topics
        let key = 0;
        let connection = 0;
        connection_map.add_connection(key, connection, initial_topics.clone());

        // make sure no topics are associated with the key
        assert_eq!(connection_map.key_to_topics.get(&key).unwrap().len(), 0);

        // subscribe to a topic
        let topics = vec![Topic::Global];
        connection_map.subscribe_key_to(key, topics.clone());

        // make sure the topic is associated with the key
        let connections = connection_map.get_connections_for_broadcast(key, topics.clone());
        assert_eq!(connections.len(), 1);
        assert!(connections.contains(&connection));

        // unsubscribe from the topic
        connection_map.unsubscribe_key_from(key, topics.clone());

        // make sure the topic is not associated with the key
        let connections = connection_map.get_connections_for_broadcast(key, topics.clone());
        assert_eq!(connections.len(), 0);
        assert!(!connections.contains(&connection));
    }
}
